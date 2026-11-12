//! A filter from a plugin file: one WGSL function, `pixel`, with everything around it written here
//! (the template in `shaders/plugin_filter_1.wgsl`). It takes a pixel of the layer and hands one
//! back; premultiplying, the selection, the alpha lock and the layer's copy are the app's.
//!
//! The file is hostile until it has held up, and a GPU can't be interrupted, so what it can do is
//! bounded before it runs:
//! - Its text may declare functions, constants and types, nothing else, and reach nothing of the
//!   template's but `v`, `layer_at` and `canvas_size`.
//! - Its loops pay as they go round: the parsed module is rewritten so each spends from a budget
//!   a pixel has, and stops when it is spent. That holds on every backend, the same code going to
//!   each. With no recursion in WGSL, and calls counted in what a loop pays, a pixel can't cost more
//!   than the budget and the straight line through the code.
//! - So the most a pixel can cost is known, and a layer is drawn in pieces of bands, each sent
//!   alone and waited for, small enough that even the worst of them is over long before an OS
//!   would step in. Between them the screen gets its turn. A filter with no loops is one piece.
//! - A small probe goes first, and finds a filter that runs out of budget before it meets the layer.
//!
//! The waits are also how a piece cut short is seen. The tablet tested stops a draw that runs past
//! 10 seconds or so and says nothing: the fence signals, the target holds what it was cleared to,
//! and the whole screen has stood still meanwhile. The third or fourth time it loses the device, and
//! every layer with it. So a piece that takes far longer than any piece can counts as cut, nothing
//! goes on from it, and the layer is put back from its copy.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use wgpu::naga::{self, Arena, BinaryOperator, Block, Expression, Function, Handle, Literal, Module, Span, Statement};

use super::{
    Canvas, FilterUniform, LAYER_FORMAT, Op, UniformData, blended_pipeline, copy_at, extent, render_pass, texture,
};
use crate::{Gpu, GpuError};

const TEMPLATE: &str = include_str!("../shaders/plugin_filter_1.wgsl");
const PICTURE_TEMPLATE: &str = include_str!("../shaders/plugin_picture_2.wgsl");

/// The most a filter's text may be, in bytes.
const MOST_TEXT: usize = 64 << 10;
/// How many controls a filter may have, and where each value is in the uniform.
const VALUE_PLACES: [&str; 8] = ["a.x", "a.y", "a.z", "a.w", "b.x", "b.y", "b.z", "b.w"];
/// What a pixel may spend in loops, in roughly simple operations, and in code that isn't in one.
const BUDGET: u64 = 1 << 16;
const MOST_STRAIGHT: u64 = 1 << 16;
/// The most one of its types may be, and one function's variables, in bytes: a pixel's memory is
/// kept per pixel, and a large array stops a shader compiler or the GPU.
const MOST_TYPE: u32 = 16 << 10;
const MOST_LOCALS: u64 = 64 << 10;
/// What a piece may cost at the most, in pixels by what a pixel can spend. A GPU like a tablet's
/// gets through about 400 billion of these a second, so this is about 20 ms there, and a GPU fifty
/// times slower still finishes inside the two seconds Windows gives.
const WORK_AT_ONCE: u64 = 1 << 33;
/// A piece that takes this long isn't one that finished: no GPU that can draw the canvas needs it.
const PIECE_LIMIT: Duration = Duration::from_millis(1500);
/// The most a whole layer may take: the app waits for it.
const WHOLE_LIMIT: Duration = Duration::from_secs(4);
/// The most a driver may take to compile one.
pub(super) const COMPILE_LIMIT: Duration = Duration::from_secs(15);
/// Parsing and compiling recurse over what the file holds.
pub(super) const BIG_STACK: usize = 64 << 20;
/// How many pixels each way the probe looks at.
const PROBE_SIZE: u32 = 16;

/// What the template's own names are, among those a filter's text can't use.
const OWN_GLOBALS: [&str; 5] = ["host_copy", "host_chosen", "host_u", "v", "host_budget"];
const OWN_ENTRIES: [&str; 3] = ["host_main", "host_probe", "host_vs"];
const OWN_PREFIX: &str = "host_";
/// The template's functions a filter may call.
const GIVEN: [&str; 2] = ["layer_at", "canvas_size"];
/// The one function it must have.
const PIXEL: &str = "pixel";
/// What an author may well call it, which WGSL keeps for itself: said so, not left to the parser.
const RESERVED: &str = "filter";

/// What one kind of plugin code is joined to and given: a filter's, or a picture's
/// (`plugin_picture.rs`). Both are checked the same way.
pub(super) struct Kind {
    template: &'static str,
    own_globals: &'static [&'static str],
    given: &'static [&'static str],
    /// The one function it must have.
    entry: &'static str,
}

const FILTER: Kind = Kind { template: TEMPLATE, own_globals: &OWN_GLOBALS, given: &GIVEN, entry: PIXEL };
pub(super) const PICTURE: Kind = Kind {
    template: PICTURE_TEMPLATE,
    own_globals: &["host_u", "host_budget"],
    given: &["hue", "picture_size"],
    entry: "picture",
};

/// A filter's code that has held up, with what its loops pay written in. Nothing has run.
#[derive(Clone, Debug)]
pub struct FilterCode {
    module: Module,
    /// The most a pixel can cost.
    worst: u64,
    loops: bool,
}

impl FilterCode {
    /// The most a pixel can cost.
    pub fn worst(&self) -> u64 {
        self.worst
    }

    pub(super) fn loops(&self) -> bool {
        self.loops
    }

    pub(super) fn into_module(self) -> Module {
        self.module
    }
}

/// `work` on a stack that parsing or compiling what a file holds can't run out of: a stack
/// overflow would end the process with nothing to catch. None if it panicked.
fn on_a_big_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::Builder::new().name("plugin filter".into()).stack_size(BIG_STACK).spawn(work).ok()?.join().ok()
}

/// Reads `text`, a filter's code, for a filter whose controls are `keys`, in the manifest's order.
/// Why not, in words for the log, if it can't be run. Nothing runs, and no GPU is needed.
pub fn read_filter(text: &str, keys: &[String]) -> Result<FilterCode, String> {
    if text.len() > MOST_TEXT {
        return Err("too long".to_owned());
    }
    let (text, keys) = (text.to_owned(), keys.to_vec());
    on_a_big_stack(move || read(&text, &keys, &FILTER)).unwrap_or_else(|| Err("reading it panicked".to_owned()))
}

/// Reads `text`, code of `kind` with no controls, as [`read_filter`] reads a filter's.
pub(super) fn read_kind(text: &str, kind: &'static Kind) -> Result<FilterCode, String> {
    if text.len() > MOST_TEXT {
        return Err("too long".to_owned());
    }
    let text = text.to_owned();
    on_a_big_stack(move || read(&text, &[], kind)).unwrap_or_else(|| Err("reading it panicked".to_owned()))
}

/// Whether `key` is a plain field name; what isn't must not go into the template.
fn plain_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The template with `keys` as its values, and `text` after all of it: there is nothing below it
/// for the text to change the meaning of.
fn joined(text: &str, keys: &[String], kind: &Kind) -> String {
    let fields: String = keys.iter().map(|key| format!("{key}: f32, ")).collect();
    let read: Vec<String> = VALUE_PLACES.iter().take(keys.len()).map(|place| format!("host_u.{place}")).collect();
    let (fields, read) =
        if keys.is_empty() { ("host_none: f32,".to_owned(), "0.0".to_owned()) } else { (fields, read.join(", ")) };
    let template = kind
        .template
        .replace("/*VALUES*/", &fields)
        .replace("/*READ*/", &read)
        .replace("/*BUDGET*/", &BUDGET.to_string());
    format!("{template}\n{text}\n")
}

/// How WGSL ends a line.
fn ends_line(c: char) -> bool {
    matches!(c, '\u{A}' | '\u{B}' | '\u{C}' | '\u{D}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// The words of WGSL `text` and how deep in braces each is, comments left out.
fn words(text: &str) -> Vec<(&str, usize)> {
    let mut found = Vec::new();
    let mut depth = 0_usize;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let next = chars.peek().map(|&(_, c)| c);
        match (c, next) {
            ('/', Some('/')) => while chars.next_if(|&(_, c)| !ends_line(c)).is_some() {},
            ('/', Some('*')) => {
                chars.next();
                let mut open = 1_usize;
                while open > 0 {
                    let Some((_, c)) = chars.next() else { break };
                    let next = chars.peek().map(|&(_, c)| c);
                    match (c, next) {
                        ('/', Some('*')) => {
                            chars.next();
                            open += 1;
                        }
                        ('*', Some('/')) => {
                            chars.next();
                            open -= 1;
                        }
                        _ => {}
                    }
                }
            }
            ('{', _) => depth += 1,
            ('}', _) => depth = depth.saturating_sub(1),
            _ if c.is_alphanumeric() || c == '_' => {
                let mut end = at + c.len_utf8();
                while let Some((i, c)) = chars.next_if(|&(_, c)| c.is_alphanumeric() || c == '_') {
                    end = i + c.len_utf8();
                }
                if let Some(word) = text.get(at..end) {
                    found.push((word, depth));
                }
            }
            _ => {}
        }
    }
    found
}

/// The names `text` declares for the whole module.
fn declared(text: &str) -> Vec<&str> {
    let found = words(text);
    let declares = |word: &str| matches!(word, "fn" | "struct" | "alias" | "const" | "override" | "var");
    found
        .windows(2)
        .filter_map(|pair| match pair {
            [(word, 0), (name, _)] if declares(word) => Some(*name),
            _ => None,
        })
        .collect()
}

/// `text` as code, checked, and what its loops pay written in.
fn read(text: &str, keys: &[String], kind: &Kind) -> Result<FilterCode, String> {
    if keys.len() > VALUE_PLACES.len() || !keys.iter().all(|key| plain_key(key)) {
        return Err("its controls can't be its values".to_owned());
    }
    // what its text declares reaches the whole module, the template too: a function of its own
    // called `mix` would take the selection's place. So none may be a word the template uses
    let template: HashSet<&str> = words(kind.template).into_iter().map(|(word, _)| word).collect();
    let names = declared(text);
    let entry = kind.entry;
    if names.contains(&RESERVED) {
        return Err(format!("{RESERVED} is a word WGSL keeps for itself: the function is called {entry}"));
    }
    if !names.contains(&entry) {
        return Err(format!("it has no {entry} function"));
    }
    let taken = names.iter().find(|name| {
        **name != entry && (template.contains(*name) || name.starts_with(OWN_PREFIX) || kind.given.contains(*name))
    });
    if let Some(name) = taken {
        return Err(format!("it declares {name}, which is the app's"));
    }
    let whole = joined(text, keys, kind);
    let mut module = naga::front::wgsl::parse_str(&whole).map_err(|e| e.emit_to_string(&whole))?;
    holds_only(&module, kind)?;
    validate(&module, &whole)?;
    let pixel = function_named(&module, entry).ok_or_else(|| format!("it has no {entry} function"))?;
    let costs = straight_costs(&module);
    let straight = costs.get(pixel.index()).copied().unwrap_or(u64::MAX);
    if straight > MOST_STRAIGHT {
        return Err(format!("it costs a pixel {straight} without a loop"));
    }
    let loops = module.functions.iter().any(|(_, function)| has_a_loop(&function.body));
    pay_in_loops(&mut module, &costs).ok_or("the template has no budget")?;
    validate(&module, &whole)?;
    Ok(FilterCode { module, worst: straight + if loops { BUDGET } else { 0 }, loops })
}

fn validate(module: &Module, text: &str) -> Result<(), String> {
    let flags = naga::valid::ValidationFlags::all();
    let capabilities = naga::valid::Capabilities::default();
    naga::valid::Validator::new(flags, capabilities).validate(module).map(|_| ()).map_err(|e| e.emit_to_string(text))
}

fn function_named(module: &Module, name: &str) -> Option<Handle<Function>> {
    module.functions.iter().find(|(_, function)| function.name.as_deref() == Some(name)).map(|(handle, _)| handle)
}

/// Whether what the template joined with a filter's (or a picture's) text has nothing but what the
/// template brought and some functions, constants and types, none of them too big.
fn holds_only(module: &Module, kind: &Kind) -> Result<(), String> {
    let mut entries: Vec<&str> = module.entry_points.iter().map(|entry| entry.name.as_str()).collect();
    entries.sort_unstable();
    if entries != OWN_ENTRIES {
        return Err("it has entry points of its own".to_owned());
    }
    let own = |name: &Option<String>| name.as_deref().is_some_and(|name| kind.own_globals.contains(&name));
    let mut globals = module.global_variables.iter();
    if module.global_variables.len() != kind.own_globals.len() || !globals.all(|(_, global)| own(&global.name)) {
        return Err("it declares global variables or resources".to_owned());
    }
    if !module.overrides.is_empty() {
        return Err("it declares overrides".to_owned());
    }
    let context = module.to_ctx();
    if module.types.iter().any(|(_, ty)| ty.inner.try_size(context).is_none_or(|size| size > MOST_TYPE)) {
        return Err("it has a type too big for a pixel to hold".to_owned());
    }
    let values = module.global_variables.iter().find(|(_, global)| global.name.as_deref() == Some("v"));
    let values = values.map(|(handle, _)| handle);
    for (_, function) in module.functions.iter() {
        let name = function.name.as_deref().unwrap_or("");
        if name.starts_with(OWN_PREFIX) || kind.given.contains(&name) {
            continue;
        }
        let locals = function.local_variables.iter();
        let bytes = locals.map(|(_, local)| module.types.get_handle(local.ty).ok());
        let bytes: u64 =
            bytes.map(|ty| u64::from(ty.and_then(|ty| ty.inner.try_size(context)).unwrap_or(u32::MAX))).sum();
        if bytes > MOST_LOCALS {
            return Err("a function's variables are too big for a pixel to hold".to_owned());
        }
        for (_, expression) in function.expressions.iter() {
            allowed_expression(expression, values)?;
        }
        allowed_statements(&function.body, module)?;
    }
    Ok(())
}

/// Only what a pure function of numbers uses: this is what leaves a filter nothing to reach, and
/// whatever a later WGSL brings stays out until it is looked at.
fn allowed_expression(expression: &Expression, values: Option<Handle<naga::GlobalVariable>>) -> Result<(), String> {
    use Expression as E;
    match expression {
        E::GlobalVariable(global) if Some(*global) != values => Err("it reaches what is the app's".to_owned()),
        E::Literal(_)
        | E::Constant(_)
        | E::ZeroValue(_)
        | E::Compose { .. }
        | E::Access { .. }
        | E::AccessIndex { .. }
        | E::Splat { .. }
        | E::Swizzle { .. }
        | E::FunctionArgument(_)
        | E::GlobalVariable(_)
        | E::LocalVariable(_)
        | E::Load { .. }
        | E::Unary { .. }
        | E::Binary { .. }
        | E::Select { .. }
        | E::Relational { .. }
        | E::Math { .. }
        | E::As { .. }
        | E::CallResult(_) => Ok(()),
        _ => Err("it uses something a filter can't".to_owned()),
    }
}

fn allowed_statements(block: &Block, module: &Module) -> Result<(), String> {
    use Statement as S;
    for statement in block.iter() {
        match statement {
            S::Block(inner) => allowed_statements(inner, module)?,
            S::If { accept, reject, .. } => {
                allowed_statements(accept, module)?;
                allowed_statements(reject, module)?;
            }
            S::Switch { cases, .. } => {
                for case in cases {
                    allowed_statements(&case.body, module)?;
                }
            }
            S::Loop { body, continuing, .. } => {
                allowed_statements(body, module)?;
                allowed_statements(continuing, module)?;
            }
            S::Call { function, .. } => {
                let called = module.functions.try_get(*function).ok().and_then(|f| f.name.as_deref());
                if called.is_none_or(|name| name.starts_with(OWN_PREFIX)) {
                    return Err("it calls what is the app's".to_owned());
                }
            }
            S::Emit(_) | S::Break | S::Continue | S::Return { .. } | S::Store { .. } => {}
            _ => return Err("it does something a filter can't".to_owned()),
        }
    }
    Ok(())
}

fn has_a_loop(block: &Block) -> bool {
    block.iter().any(|statement| match statement {
        Statement::Loop { .. } => true,
        Statement::Block(inner) => has_a_loop(inner),
        Statement::If { accept, reject, .. } => has_a_loop(accept) || has_a_loop(reject),
        Statement::Switch { cases, .. } => cases.iter().any(|case| has_a_loop(&case.body)),
        _ => false,
    })
}

fn expression_cost(expression: &Expression) -> u64 {
    match expression {
        Expression::Math { .. } => 4,
        Expression::ImageLoad { .. } => 16,
        _ => 1,
    }
}

/// What `block` costs to go through once, its loops left out: each pays for itself as it goes
/// round. `costs` are the functions' before this one, by index: a function only calls those.
fn block_cost(block: &Block, expressions: &Arena<Expression>, costs: &[u64]) -> u64 {
    let total = |blocks: &[&Block]| -> u64 {
        blocks.iter().map(|block| block_cost(block, expressions, costs)).fold(1, u64::saturating_add)
    };
    block.iter().fold(0, |cost, statement| {
        cost.saturating_add(match statement {
            Statement::Emit(range) => {
                range.clone().map(|e| expressions.try_get(e).map_or(1, expression_cost)).fold(0, u64::saturating_add)
            }
            Statement::Block(inner) => total(&[inner]),
            Statement::If { accept, reject, .. } => total(&[accept, reject]),
            Statement::Switch { cases, .. } => {
                let bodies: Vec<&Block> = cases.iter().map(|case| &case.body).collect();
                total(&bodies)
            }
            Statement::Call { function, .. } => {
                costs.get(function.index()).copied().unwrap_or(u64::MAX).saturating_add(1)
            }
            _ => 1,
        })
    })
}

/// What each function costs to go through once, by index.
fn straight_costs(module: &Module) -> Vec<u64> {
    let mut costs = Vec::with_capacity(module.functions.len());
    for (_, function) in module.functions.iter() {
        let cost = block_cost(&function.body, &function.expressions, &costs);
        costs.push(cost);
    }
    costs
}

/// Makes every loop in `module` pay. The filter's text can't touch the budget: it reaches no global
/// but its values.
fn pay_in_loops(module: &mut Module, costs: &[u64]) -> Option<()> {
    let budget = module.global_variables.iter().find(|(_, g)| g.name.as_deref() == Some("host_budget"))?.0;
    for (_, function) in module.functions.iter_mut() {
        pay_in(&mut function.body, &mut function.expressions, costs, budget);
    }
    Some(())
}

fn pay_in(block: &mut Block, expressions: &mut Arena<Expression>, costs: &[u64], budget: Handle<naga::GlobalVariable>) {
    for (statement, _) in block.span_iter_mut() {
        match statement {
            Statement::Block(inner) => pay_in(inner, expressions, costs, budget),
            Statement::If { accept, reject, .. } => {
                pay_in(accept, expressions, costs, budget);
                pay_in(reject, expressions, costs, budget);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    pay_in(&mut case.body, expressions, costs, budget);
                }
            }
            Statement::Loop { body, continuing, .. } => {
                let round =
                    block_cost(body, expressions, costs).saturating_add(block_cost(continuing, expressions, costs));
                pay_in(body, expressions, costs, budget);
                pay_in(continuing, expressions, costs, budget);
                let rest = std::mem::replace(body, paying(expressions, budget, round));
                body.extend_block(rest);
            }
            _ => {}
        }
    }
}

/// The head of a loop that costs `round` to go round: the budget goes down by that, and if there
/// isn't that much it is marked as spent and the loop ends. It never goes below -1, so no amount of
/// loops after it can wrap it round.
fn paying(expressions: &mut Arena<Expression>, budget: Handle<naga::GlobalVariable>, round: u64) -> Block {
    let round = i32::try_from(round.saturating_add(1)).unwrap_or(i32::MAX);
    let mut add = |expression| expressions.append(expression, Span::UNDEFINED);
    let pointer = add(Expression::GlobalVariable(budget));
    let cost = add(Expression::Literal(Literal::I32(round)));
    let spent_mark = add(Expression::Literal(Literal::I32(-1)));
    let left = add(Expression::Load { pointer });
    let short = add(Expression::Binary { op: BinaryOperator::Less, left, right: cost });
    let next = add(Expression::Binary { op: BinaryOperator::Subtract, left, right: cost });
    let mut stop = Block::new();
    stop.push(Statement::Store { pointer, value: spent_mark }, Span::UNDEFINED);
    stop.push(Statement::Break, Span::UNDEFINED);
    let mut head = Block::new();
    head.push(Statement::Emit(naga::Range::new_from_bounds(left, short)), Span::UNDEFINED);
    head.push(Statement::If { condition: short, accept: stop, reject: Block::new() }, Span::UNDEFINED);
    head.push(Statement::Emit(naga::Range::new_from_bounds(next, next)), Span::UNDEFINED);
    head.push(Statement::Store { pointer, value: next }, Span::UNDEFINED);
    head
}

/// A filter ready to draw on this device.
pub struct FilterProgram {
    draw: wgpu::RenderPipeline,
    probe: wgpu::RenderPipeline,
    probe_target: wgpu::Texture,
    worst: u64,
    loops: bool,
}

/// How a filter's draw went.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterRun {
    /// The whole layer, in this long.
    Drew(Duration),
    /// Run out of its budget on a pixel looked at: it never ends, or does too much for a pixel.
    /// The layer is as it was.
    Spent,
    /// A piece took longer than one can, so it may have been stopped: the layer is as it was.
    Cut,
    /// It went on too long for a canvas this big. The layer is as it was.
    TooLong,
    /// The device was lost: nothing it held can be read.
    Lost,
}

impl Gpu {
    /// Compiles `code` for this device: on a stack of its own, as the driver's compiler sees what
    /// the file holds, and waited for only so long. Its errors come back here, not to the device's
    /// count. What a driver does on it nothing can catch: see the core's record of it.
    pub fn make_filter(&self, code: FilterCode) -> Result<Arc<FilterProgram>, GpuError> {
        let (done, came) = mpsc::channel();
        let gpu = self.clone();
        let started = std::thread::Builder::new().name("plugin filter".into()).stack_size(BIG_STACK).spawn(move || {
            let _ = done.send(gpu.filter_pipelines(code));
        });
        started.map_err(|e| GpuError::Failed(e.to_string()))?;
        came.recv_timeout(COMPILE_LIMIT).map_err(|e| GpuError::Failed(format!("compiling a filter: {e}")))?
    }

    fn filter_pipelines(&self, code: FilterCode) -> Result<Arc<FilterProgram>, GpuError> {
        let layout = self.canvas_pipelines().filter_layout.clone();
        let (worst, loops) = (code.worst, code.loops);
        self.checked(|| {
            let device = &self.device;
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("plugin filter"),
                source: wgpu::ShaderSource::Naga(Cow::Owned(code.module)),
            });
            let pipeline = |entry: &str| {
                // a plugin's own code: not kept for the next run
                blended_pipeline(
                    device,
                    None,
                    &layout,
                    &module,
                    "plugin filter",
                    ("host_vs", entry),
                    LAYER_FORMAT,
                    None,
                )
            };
            let probe_target = texture(device, "plugin filter probe", PROBE_SIZE, PROBE_SIZE, LAYER_FORMAT);
            Arc::new(FilterProgram {
                draw: pipeline("host_main"),
                probe: pipeline("host_probe"),
                probe_target,
                worst,
                loops,
            })
        })
    }
}

/// How many rows of a canvas `width` across go in a piece, for a filter at `worst` a pixel.
fn rows_at_once(width: u32, worst: u64) -> u32 {
    let row = u64::from(width.max(1)).saturating_mul(worst.max(1));
    u32::try_from(WORK_AT_ONCE / row).unwrap_or(u32::MAX).max(1)
}

/// The values a filter's draw is given.
pub(super) struct FilterValues {
    pub(super) program: Arc<FilterProgram>,
    pub(super) values: [f32; VALUE_PLACES.len()],
    pub(super) alpha_locked: bool,
}

impl Canvas {
    /// Redraws the layer being edited (begun with [`Self::begin_transform`]) from its copy through
    /// `program` with `values` (each control's, in order), as [`Self::set_filter`] does. Sent in
    /// pieces and waited for, so it comes with [`Self::take_filter_run`] to say how that went.
    pub fn set_plugin_filter(&mut self, program: &Arc<FilterProgram>, values: &[f32], alpha_locked: bool) {
        let mut padded = [0.0; VALUE_PLACES.len()];
        for (kept, value) in padded.iter_mut().zip(values) {
            *kept = *value;
        }
        let values = FilterValues { program: Arc::clone(program), values: padded, alpha_locked };
        self.replace_placement(Op::PluginFilter(values));
    }

    /// How the last filter's draw went, once.
    pub fn take_filter_run(&mut self) -> Option<FilterRun> {
        self.filter_run.take()
    }

    /// Tells of a filter's draw as if it had gone so: for tests of what comes after.
    #[doc(hidden)]
    pub fn tell_filter_run(&mut self, run: FilterRun) {
        self.filter_run = Some(run);
    }

    /// Draws a filter: all of it, or the layer goes back as it was and the run says why.
    pub(super) fn plugin_filter(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        filter: &FilterValues,
    ) {
        let Some(t) = &self.transforming else { return };
        let Some(layer) = self.layers.get(&t.layer) else { return };
        let (w, h) = (self.width as f32, self.height as f32);
        let chosen = if t.masked { 1.0 } else { 0.0 };
        let locked = if filter.alpha_locked { 1.0 } else { 0.0 };
        let [a @ .., _, _, _, _] = filter.values;
        let [_, _, _, _, b @ ..] = filter.values;
        let uniform =
            FilterUniform { a, b, size: [w, h, chosen, locked], c: [PROBE_SIZE as f32, PROBE_SIZE as f32, 0.0, 0.0] };
        let draw = |encoder: &mut wgpu::CommandEncoder,
                    uniforms: &mut UniformData,
                    target: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    load: wgpu::LoadOp<wgpu::Color>,
                    rows: Option<(u32, u32)>| {
            let offset = uniforms.push(&uniform);
            let mut pass = render_pass(encoder, "plugin filter", target, load);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &t.bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.set_bind_group(2, &t.before_bind, &[]);
            if let Some((from, count)) = rows {
                pass.set_scissor_rect(0, from, self.width, count);
            }
            pass.draw(0..3, 0..1);
        };
        // waits for what was sent; whether it finished and the device stands
        let finished = |gpu: &Gpu| -> Result<(), FilterRun> {
            let waited = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(PIECE_LIMIT) });
            if gpu.is_lost() {
                return Err(FilterRun::Lost);
            }
            waited.map(|_| ()).map_err(|_| FilterRun::Cut)
        };

        // what came before goes first, and however long that takes isn't held against the filter
        self.send_now(gpu, encoder, uniforms);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let mut run = || -> Result<Duration, FilterRun> {
            if gpu.is_lost() {
                return Err(FilterRun::Lost);
            }
            let started = Instant::now();
            if filter.program.loops {
                let probe = &filter.program.probe_target;
                let target = probe.create_view(&wgpu::TextureViewDescriptor::default());
                let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
                draw(encoder, uniforms, &target, &filter.program.probe, clear, None);
                self.send_now(gpu, encoder, uniforms);
                finished(gpu)?;
                if started.elapsed() >= PIECE_LIMIT {
                    return Err(FilterRun::Cut);
                }
                let looked = super::read_texture(gpu, probe, PROBE_SIZE, PROBE_SIZE);
                let looked = looked.map_err(|_| if gpu.is_lost() { FilterRun::Lost } else { FilterRun::Cut })?;
                if looked.as_chunks::<4>().0.iter().any(|[red, ..]| *red > 127) {
                    return Err(FilterRun::Spent);
                }
            }
            let rows = rows_at_once(self.width, filter.program.worst);
            let mut from = 0;
            while from < self.height {
                let count = rows.min(self.height - from);
                let sent = Instant::now();
                let piece = Some((from, count));
                draw(encoder, uniforms, &layer.view, &filter.program.draw, wgpu::LoadOp::Load, piece);
                self.send_now(gpu, encoder, uniforms);
                finished(gpu)?;
                if sent.elapsed() >= PIECE_LIMIT {
                    return Err(FilterRun::Cut);
                }
                if started.elapsed() >= WHOLE_LIMIT {
                    return Err(FilterRun::TooLong);
                }
                from += count;
            }
            Ok(started.elapsed())
        };
        let run = run();
        // nothing of a filter that didn't finish stays
        if run.is_err() {
            encoder.copy_texture_to_texture(
                copy_at(&t.before, 0, 0),
                copy_at(&layer.texture, 0, 0),
                extent(self.width, self.height),
            );
        }
        self.filter_run = Some(run.map_or_else(|failed| failed, FilterRun::Drew));
        self.dirty = self.full();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &str = "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return colour; }";

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn read_plain(text: &str) -> Result<FilterCode, String> {
        read_filter(text, &[])
    }

    fn filter_doing(body: &str) -> String {
        format!("fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {{ {body} }}")
    }

    /// Whether every loop in `block` starts by paying.
    fn every_loop_pays(block: &Block) -> bool {
        block.iter().all(|statement| match statement {
            Statement::Loop { body, continuing, .. } => {
                let mut first = body.iter();
                let pays = matches!(first.next(), Some(Statement::Emit(_)))
                    && matches!(first.next(), Some(Statement::If { accept, .. })
                        if matches!(accept.get(1), Some(Statement::Break)));
                pays && every_loop_pays(body) && every_loop_pays(continuing)
            }
            Statement::Block(inner) => every_loop_pays(inner),
            Statement::If { accept, reject, .. } => every_loop_pays(accept) && every_loop_pays(reject),
            Statement::Switch { cases, .. } => cases.iter().all(|case| every_loop_pays(&case.body)),
            _ => true,
        })
    }

    fn all_pay(code: &FilterCode) -> bool {
        code.module.functions.iter().all(|(_, function)| every_loop_pays(&function.body))
    }

    #[test]
    fn a_plain_filter_reads_and_costs_little_with_no_loop() {
        let code = read_plain(PLAIN).unwrap();
        assert!(!code.loops);
        assert!(code.worst() < 100, "{}", code.worst());
        // it can use its values, and what it is given
        let text =
            filter_doing("let n = layer_at(at + vec2<f32>(1.0, 0.0)); return mix(colour, n, v.cut * canvas_size().x);");
        assert!(read_filter(&text, &keys(&["cut"])).is_ok());
        // but not one it doesn't have
        assert!(read_filter(&text, &keys(&["amount"])).is_err());
        // a helper of its own, a constant, a type of its own
        let text = "struct Two { a: f32, b: f32 }\nconst half = 0.5;\nfn twice(x: f32) -> f32 { return x * 2.0; }\n";
        let text = format!("{text}{}", filter_doing("let t = Two(half, twice(0.25)); return colour * t.b;"));
        assert!(read_plain(&text).is_ok());
    }

    #[test]
    fn what_doesnt_fit_the_shape_is_refused() {
        // what won't compile, and what isn't the one function it should be
        assert!(read_plain("fn pixel(").is_err());
        assert!(read_plain("").is_err());
        assert!(read_plain("fn pixel(colour: vec4<f32>) -> vec4<f32> { return colour; }").is_err());
        assert!(read_plain("fn pixel(colour: vec4<f32>, at: vec2<f32>) -> f32 { return 1.0; }").is_err());
        // its own entry points, bindings and globals of any kind
        let entry = format!("{PLAIN}\n@fragment fn own() -> @location(0) vec4<f32> {{ return vec4<f32>(1.0); }}");
        assert!(read_plain(&entry).is_err());
        let compute = format!("{PLAIN}\n@compute @workgroup_size(1) fn own() {{}}");
        assert!(read_plain(&compute).is_err());
        for global in [
            "@group(3) @binding(0) var<uniform> mine: vec4<f32>;",
            "@group(0) @binding(9) var mine: texture_2d<f32>;",
            "@group(0) @binding(9) var<storage, read_write> mine: array<f32>;",
            "var<private> mine: f32;",
            "var<workgroup> mine: f32;",
            "override mine: f32 = 1.0;",
        ] {
            assert!(read_plain(&format!("{global}\n{PLAIN}")).is_err(), "{global}");
        }
        // nor as a stranger with more than it is given
        assert!(read_plain(&"x".repeat(MOST_TEXT + 1)).is_err());
        assert!(read_filter(PLAIN, &keys(&["a", "b", "c", "d", "e", "f", "g", "h", "i"])).is_err());
        assert!(read_filter(PLAIN, &keys(&["a: f32, b"])).is_err());
    }

    #[test]
    fn the_function_is_called_pixel_and_a_file_is_told_so() {
        assert!(read_plain(PLAIN).is_ok());
        // filter is a word WGSL keeps for itself: said in so many words, not by the parser
        let named = PLAIN.replace("fn pixel", "fn filter");
        assert!(read_plain(&named).unwrap_err().contains("function is called pixel"));
        let helper = format!("fn filter() {{}}\n{PLAIN}");
        assert!(read_plain(&helper).unwrap_err().contains("function is called pixel"));
        // and nowhere else does the word get past it
        assert!(read_plain(&filter_doing("let filter = 1.0; return colour * filter;")).is_err());
        // a file with neither
        let other = PLAIN.replace("fn pixel", "fn apply");
        assert!(read_plain(&other).unwrap_err().contains("no pixel function"));
    }

    #[test]
    fn it_reaches_nothing_of_the_apps() {
        for body in [
            "host_budget = 1000000; return colour;",
            "return textureLoad(host_copy, vec2<i32>(0), 0);",
            "return textureLoad(host_chosen, vec2<i32>(0), 0);",
            "return host_u.a;",
            "return host_straight(colour);",
            "return host_filtered(vec2<i32>(0));",
            "discard;",
            "workgroupBarrier(); return colour;",
            "return colour * dpdx(colour.r);",
        ] {
            assert!(read_plain(&filter_doing(body)).is_err(), "{body}");
        }
    }

    #[test]
    fn it_cant_change_what_the_template_means() {
        for own in [
            "fn mix(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> { return b; }",
            "fn clamp(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>) -> vec4<f32> { return a; }",
            "fn textureLoad() {}",
            "alias f32 = i32;",
            "alias vec4 = vec3;",
            "const select = 1;",
            "struct all { a: f32 }",
            // however it is hidden
            "/* */ fn /* */ mix(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> { return b; }",
            "// a comment\u{2028}fn mix(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> { return b; }",
            // and what the app gives it, or may give it in the next one
            "fn layer_at() {}",
            "fn host_new() {}",
        ] {
            assert!(read_plain(&format!("{own}\n{PLAIN}")).is_err(), "{own}");
        }
        // inside a function, a name is its own business
        assert!(read_plain(&filter_doing("let mix = 1.0; return colour * mix;")).is_ok());
    }

    #[test]
    fn what_a_pixel_cant_hold_is_refused() {
        // an array too big, as a type however it is reached
        for body in [
            "var a: array<f32, 100000>; a[0] = 1.0; return colour * a[0];",
            "let a = array<array<f32, 1000>, 1000>(); return colour * a[0][0];",
        ] {
            assert!(read_plain(&filter_doing(body)).is_err(), "{body}");
        }
        // many that each fit
        let many: String = (0..8).map(|i| format!("var a{i}: array<f32, 4000>; a{i}[0] = 1.0;")).collect();
        assert!(read_plain(&filter_doing(&format!("{many} return colour * a0[0];"))).is_err());
    }

    #[test]
    fn calls_that_multiply_are_counted() {
        // each calling the one before ten times: six deep is a million calls, with no loop in it
        let mut text = String::from("fn f0(x: f32) -> f32 { return x + 1.0; }\n");
        for i in 1..7 {
            let calls: String = (0..10).map(|_| format!("y = f{}(y);", i - 1)).collect();
            text += &format!("fn f{i}(x: f32) -> f32 {{ var y = x; {calls} return y; }}\n");
        }
        let text = format!("{text}{}", filter_doing("return colour * f6(1.0);"));
        assert!(read_plain(&text).unwrap_err().contains("costs a pixel"));
    }

    #[test]
    fn every_loop_pays_however_it_is_written() {
        for body in [
            "loop { if (colour.r > 2.0) { break; } } return colour;",
            "var i = 0; while (i < 10) { i += 1; } return colour;",
            "for (var i = 0; i < 10; i += 1) { for (var j = 0; j < 10; j += 1) { } } return colour;",
            "var i = 0; loop { i += 1; continuing { break if i > 3; } } return colour;",
            "var i = 0; loop { switch i { case 0: { loop { i += 1; if (i > 1) { break; } } } default: { break; } } i += 1; if (i > 3) { break; } } return colour;",
            "if (colour.r > 0.0) { var i = 0; loop { i += 1; if (i > 1) { break; } } } return colour;",
            "var i = 0; loop { i += 1; if (i > 1) { break; } continuing { var j = 0; loop { j += 1; if (j > 1) { break; } } } } return colour;",
        ] {
            let code = read_plain(&filter_doing(body)).unwrap();
            assert!(code.loops, "{body}");
            assert!(all_pay(&code), "{body}");
            assert!(code.worst() >= BUDGET, "{body}");
        }
        // in a helper too
        let text = format!("fn spin() {{ loop {{ }} }}\n{}", filter_doing("spin(); return colour;"));
        let code = read_plain(&text).unwrap();
        assert!(code.loops && all_pay(&code));
    }

    #[test]
    fn a_loop_pays_for_what_it_calls() {
        let mut costly = String::from("fn costly(x: f32) -> f32 { var y = x;");
        costly += &"y = sin(y);".repeat(100);
        costly += " return y; }\n";
        let text = format!(
            "{costly}{}",
            filter_doing("var x = colour.r; loop { x = costly(x); if (x > 2.0) { break; } } return colour * x;")
        );
        let code = read_plain(&text).unwrap();
        let module = &code.module;
        let pixel = function_named(module, PIXEL).unwrap();
        let function = module.functions.try_get(pixel).unwrap();
        let Some(Statement::Loop { body, .. }) =
            function.body.iter().find(|statement| matches!(statement, Statement::Loop { .. }))
        else {
            panic!("no loop");
        };
        let Some(Statement::Emit(range)) = body.iter().next() else { panic!("no head") };
        let cost = range.clone().find_map(|e| match function.expressions.try_get(e) {
            Ok(Expression::Binary { op: BinaryOperator::Less, right, .. }) => {
                match function.expressions.try_get(*right) {
                    Ok(Expression::Literal(Literal::I32(cost))) => Some(*cost),
                    _ => None,
                }
            }
            _ => None,
        });
        assert!(cost.unwrap() > 400, "{cost:?}");
    }

    #[test]
    fn the_same_code_goes_to_each_backend_with_its_loops_paying() {
        use naga::back::glsl;
        let code = read_plain(&filter_doing("loop { if (colour.r > 2.0) { break; } } return colour;")).unwrap();
        let flags = naga::valid::ValidationFlags::all();
        let info = naga::valid::Validator::new(flags, naga::valid::Capabilities::default());
        let info = { info }.validate(&code.module).unwrap();
        let mut out = String::new();
        let options = glsl::Options { version: glsl::Version::new_gles(310), ..Default::default() };
        let pipeline = glsl::PipelineOptions {
            shader_stage: naga::ShaderStage::Fragment,
            entry_point: "host_main".to_owned(),
            multiview: None,
        };
        let policies = naga::proc::BoundsCheckPolicies::default();
        let mut writer = glsl::Writer::new(&mut out, &code.module, &info, &options, &pipeline, policies).unwrap();
        writer.write().unwrap();
        // the GLSL writer has no loop bounding of its own: this is what stops one there
        assert!(out.contains("65536"), "{out}");
        assert!(out.matches("break;").count() >= 2, "{out}");
    }

    #[test]
    fn pieces_are_as_small_as_the_worst_a_pixel_can_cost() {
        // no loop: a large layer goes at once
        assert!(rows_at_once(16384, 100) >= 5000);
        // with one: a band, of no less than a row
        let rows = rows_at_once(2048, BUDGET + 100);
        assert!((32..=64).contains(&rows), "{rows}");
        assert_eq!(rows_at_once(u32::MAX, u64::MAX), 1);
        assert_eq!(rows_at_once(0, 0), u32::try_from(WORK_AT_ONCE).unwrap_or(u32::MAX));
    }
}
