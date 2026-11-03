//! A PDF's pages drawn by the system (Windows' own PDF renderer, a Mac's Core Graphics; elsewhere
//! poppler's pdftoppm), to bring one in as an artwork: each page a PNG on white, fitted into the
//! first page's size.

use std::path::Path;

/// The resolution a PDF's pages are drawn at.
pub const DPI: f32 = 300.0;
/// The most a canvas side made from a PDF may be (pixels).
const MOST_SIDE: f32 = 4096.0;

/// What is said where PDFs are drawn by poppler's tools and they aren't installed.
pub const NO_POPPLER: &str = "Opening a PDF needs pdftoppm: install poppler-utils from your system's packages.";

/// The canvas a PDF whose first page is `first` points (wide, tall) comes in at: [`DPI`], its
/// longer side within [`MOST_SIDE`].
fn canvas_size((first_w, first_h): (f32, f32)) -> (u32, u32) {
    let scale = (DPI / 72.0).min(MOST_SIDE / first_w.max(first_h).max(1.0));
    (((first_w * scale).round() as u32).max(1), ((first_h * scale).round() as u32).max(1))
}

/// The size a page of `page` points is drawn at, fitted into the canvas `w` × `h`.
fn fitted((pw, ph): (f32, f32), w: u32, h: u32) -> (u32, u32) {
    let fit = (w as f32 / pw.max(1.0)).min(h as f32 / ph.max(1.0));
    (((pw * fit).round() as u32).clamp(1, w), ((ph * fit).round() as u32).clamp(1, h))
}

/// A page drawn at (about) `dw` × `dh` saved as `out`: brought to that size, in the middle of a
/// white canvas `w` × `h`.
fn save_on_white(
    mut drawn: image::RgbaImage,
    (dw, dh): (u32, u32),
    w: u32,
    h: u32,
    out: &Path,
) -> Result<(), image::ImageError> {
    if drawn.dimensions() != (dw, dh) {
        drawn = image::imageops::resize(&drawn, dw, dh, image::imageops::FilterType::CatmullRom);
    }
    let mut canvas = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    let (x, y) = (w.saturating_sub(drawn.width()) / 2, h.saturating_sub(drawn.height()) / 2);
    image::imageops::overlay(&mut canvas, &drawn, i64::from(x), i64::from(y));
    canvas.save(out)
}

/// Each page's size in points from pdfinfo's listing of them (`Page    1 size: 612 x 792 pts`),
/// a page turned a quarter (`Page    1 rot:  90`) taken as drawn, its sides swapped.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn page_sizes(listing: &str) -> Vec<(f32, f32)> {
    let mut sizes: Vec<(f32, f32)> = Vec::new();
    for line in listing.lines() {
        let Some(rest) = line.strip_prefix("Page") else { continue };
        let rest = rest.trim_start();
        let Some((_, what)) = rest.split_once(char::is_whitespace) else { continue };
        let what = what.trim_start();
        if let Some(size) = what.strip_prefix("size:") {
            let mut numbers = size.split_whitespace().filter_map(|t| t.parse::<f32>().ok());
            if let (Some(w), Some(h)) = (numbers.next(), numbers.next()) {
                sizes.push((w, h));
            }
        } else if let Some(turn) = what.strip_prefix("rot:")
            && let Ok(degrees) = turn.trim().parse::<i32>()
            && degrees.rem_euclid(180) == 90
            && let Some(last) = sizes.last_mut()
        {
            *last = (last.1, last.0);
        }
    }
    sizes
}

/// Draws the PDF at `path` into the folder `pages` as `page-N.png` (N from 1), each on white,
/// fitted into the middle of the first page's size at [`DPI`] (its longer side within 4096):
/// the number of pages and that size. None are drawn if a canvas that size holds fewer layers.
#[cfg(windows)]
pub fn draw_pages(path: &Path, pages: &Path) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::UI::Color;
    use windows::core::HSTRING;

    // the system wants a whole path, with backslashes
    let whole = std::path::absolute(path)?.to_string_lossy().replace('/', "\\");
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(whole))?.join()?;
    let pdf = PdfDocument::LoadFromFileAsync(&file)?.join()?;
    let count = pdf.PageCount()?;
    // a page's size in points (the system gives it at 96 to the inch, a PDF's at 72)
    let points = |page: &PdfPage| -> windows::core::Result<(f32, f32)> {
        let size = page.Size()?;
        Ok((size.Width * 0.75, size.Height * 0.75))
    };
    let (w, h) = canvas_size(points(&pdf.GetPage(0)?)?);
    // more pages than a canvas this size holds as layers: none drawn, as none will be taken
    if count > procraft_core::layers_for(w, h) {
        return Ok((count, w, h));
    }
    for i in 0..count {
        let page = pdf.GetPage(i)?;
        let (dw, dh) = fitted(points(&page)?, w, h);
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth(dw)?;
        options.SetDestinationHeight(dh)?;
        options.SetBackgroundColor(Color { A: 255, R: 255, G: 255, B: 255 })?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)?.join()?;
        let size = u32::try_from(stream.Size()?)?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.join()?;
        let mut png = vec![0u8; size as usize];
        reader.ReadBytes(&mut png)?;
        // in the middle of the canvas, on white (the system draws at the screen's scale, 1.4
        // times as many pixels at 140%: brought to the size asked for)
        let drawn = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.to_rgba8();
        save_on_white(drawn, (dw, dh), w, h, &pages.join(format!("page-{}.png", i + 1)))?;
    }
    Ok((count, w, h))
}

/// As Windows' does, by a Mac's own PDF renderer (Core Graphics).
#[cfg(target_os = "macos")]
pub fn draw_pages(path: &Path, pages: &Path) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    system::draw_pages(path, pages)
}

/// Core Graphics' calls, unsafe as the system's are where they take pointers.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod system {
    use std::error::Error;
    use std::path::Path;
    use std::ptr::NonNull;

    use objc2_core_foundation::{CFURL, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo, CGInterpolationQuality,
        CGPDFBox, CGPDFDocument, CGPDFPage, kCGColorSpaceSRGB,
    };

    pub fn draw_pages(path: &Path, pages: &Path) -> Result<(u32, u32, u32), Box<dyn Error>> {
        let url = CFURL::from_file_path(path).ok_or("not a file's path")?;
        let pdf = CGPDFDocument::with_url(Some(&url)).ok_or("not a PDF")?;
        // one locked only against printing or copying opens with no password
        // SAFETY: the password is an empty string, ended as the call wants
        let unlocked = CGPDFDocument::is_unlocked(Some(&pdf))
            || unsafe { CGPDFDocument::unlock_with_password(Some(&pdf), NonNull::from(c"").cast()) };
        if !unlocked {
            return Err("the PDF is locked with a password".into());
        }
        let count = u32::try_from(CGPDFDocument::number_of_pages(Some(&pdf)))?;
        let page = |n: u32| CGPDFDocument::page(Some(&pdf), n as usize).ok_or("a page is missing");
        let (w, h) = super::canvas_size(points(&*page(1)?));
        // more pages than a canvas this size holds as layers: none drawn, as none will be taken
        if count > procraft_core::layers_for(w, h) {
            return Ok((count, w, h));
        }
        for n in 1..=count {
            let page = page(n)?;
            let size = super::fitted(points(&page), w, h);
            let drawn = draw(&page, size)?;
            super::save_on_white(drawn, size, w, h, &pages.join(format!("page-{n}.png")))?;
        }
        Ok((count, w, h))
    }

    /// A page's size in points as it shows: its crop box, turned as the page says.
    fn points(page: &CGPDFPage) -> (f32, f32) {
        let crop = CGPDFPage::box_rect(Some(page), CGPDFBox::CropBox);
        let (w, h) = (crop.size.width.abs() as f32, crop.size.height.abs() as f32);
        if CGPDFPage::rotation_angle(Some(page)).rem_euclid(180) == 90 { (h, w) } else { (w, h) }
    }

    /// `page` drawn at `dw` × `dh` on white.
    fn draw(page: &CGPDFPage, (dw, dh): (u32, u32)) -> Result<image::RgbaImage, Box<dyn Error>> {
        let mut pixels = vec![0u8; dw as usize * dh as usize * 4];
        // SAFETY: the system's constant name
        let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or("no sRGB colour space")?;
        // RGBA in that order, each a byte
        let layout = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        // SAFETY: the context draws into `pixels` (`dw` × `dh`, 4 bytes each), which outlive it
        let context = unsafe {
            CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                dw as usize,
                dh as usize,
                8,
                dw as usize * 4,
                Some(&srgb),
                layout,
            )
        }
        .ok_or("no bitmap to draw the page into")?;
        let c = Some(&*context);
        let whole = CGRect::new(CGPoint::ZERO, CGSize::new(f64::from(dw), f64::from(dh)));
        CGContext::set_rgb_fill_color(c, 1.0, 1.0, 1.0, 1.0);
        CGContext::fill_rect(c, whole);
        CGContext::set_interpolation_quality(c, CGInterpolationQuality::High);
        // the page turned as it says and placed at its own size (the system's transform doesn't
        // scale up), then scaled to the bitmap
        let (pw, ph) = points(page);
        CGContext::scale_ctm(c, f64::from(dw) / f64::from(pw.max(1.0)), f64::from(dh) / f64::from(ph.max(1.0)));
        let shown = CGRect::new(CGPoint::ZERO, CGSize::new(f64::from(pw), f64::from(ph)));
        CGContext::concat_ctm(c, CGPDFPage::drawing_transform(Some(page), CGPDFBox::CropBox, shown, 0, true));
        CGContext::draw_pdf_page(c, Some(page));
        drop(context);
        // drawn over white throughout: as straight as it is premultiplied
        image::RgbaImage::from_raw(dw, dh, pixels).ok_or_else(|| "the page's pixels came out short".into())
    }
}

/// As Windows' does, by poppler's tools: `pdfinfo` for the pages and their sizes, `pdftoppm` to
/// draw each.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn draw_pages(path: &Path, pages: &Path) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    use std::process::Command;
    let run = |command: &mut Command| -> Result<std::process::Output, Box<dyn std::error::Error>> {
        let out = command.output().map_err(|e| -> Box<dyn std::error::Error> {
            if e.kind() == std::io::ErrorKind::NotFound { NO_POPPLER.into() } else { e.into() }
        })?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned().into());
        }
        Ok(out)
    };
    let info = run(Command::new("pdfinfo").arg(path))?;
    let count: u32 = String::from_utf8_lossy(&info.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Pages:").and_then(|n| n.trim().parse().ok()))
        .ok_or("no pages")?;
    let listing = run(Command::new("pdfinfo").args(["-f", "1", "-l", &count.to_string()]).arg(path))?;
    let sizes = page_sizes(&String::from_utf8_lossy(&listing.stdout));
    let &first = sizes.first().ok_or("no pages")?;
    let (w, h) = canvas_size(first);
    // more pages than a canvas this size holds as layers: none drawn, as none will be taken
    if count > procraft_core::layers_for(w, h) {
        return Ok((count, w, h));
    }
    for (i, &size) in sizes.iter().enumerate() {
        let (n, (dw, dh)) = ((i + 1).to_string(), fitted(size, w, h));
        let drawn = pages.join(format!("drawn-{n}"));
        run(Command::new("pdftoppm")
            .args(["-f", &n, "-l", &n, "-singlefile", "-png"])
            .args(["-scale-to-x", &dw.to_string(), "-scale-to-y", &dh.to_string()])
            .arg(path)
            .arg(&drawn))?;
        let png = drawn.with_extension("png");
        let picture = image::open(&png)?.to_rgba8();
        let _ = std::fs::remove_file(&png);
        save_on_white(picture, (dw, dh), w, h, &pages.join(format!("page-{n}.png")))?;
    }
    Ok((count, w, h))
}

#[cfg(test)]
mod sizing_tests {
    #[test]
    fn pdfinfo_s_pages_are_read_with_their_sizes_and_turns() {
        let listing = "Page    1 size: 612 x 792 pts (letter)\nPage    1 rot:  0\n\
                       Page    2 size: 595.28 x 841.89 pts (A4)\nPage    2 rot:  90\n\
                       Page   10 size: 144 x 72 pts\nPage   10 rot:  270\nPages:          3\n";
        assert_eq!(super::page_sizes(listing), [(612.0, 792.0), (841.89, 595.28), (72.0, 144.0)]);
        assert!(super::page_sizes("").is_empty());
    }

    #[test]
    fn a_pdf_comes_in_at_300_dpi_within_4096_and_its_pages_fitted() {
        // letter at 300 dpi; a poster capped at 4096 on its longer side
        assert_eq!(super::canvas_size((612.0, 792.0)), (2550, 3300));
        assert_eq!(super::canvas_size((2000.0, 1000.0)), (4096, 2048));
        // a landscape page into a portrait canvas: as wide as it, centred down it
        assert_eq!(super::fitted((792.0, 612.0), 2550, 3300), (2550, 1970));
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    #[test]
    fn a_pdf_s_pages_are_drawn_on_white_at_its_size() {
        // two pages 144 points square (2 inches at 72 dpi): red, then blue
        let page = |rgb: [u8; 3]| -> Vec<u8> { [rgb[0], rgb[1], rgb[2], 255].repeat(144 * 144) };
        let (red, blue) = (page([255, 0, 0]), page([0, 0, 255]));
        let bytes = procraft_engine::formats::pdf(144, 144, 72.0, &[], &[&red, &blue], 100, None);
        let dir = std::env::temp_dir().join(format!("procraft-pdf-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pages = dir.join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        let file = dir.join("two.pdf");
        std::fs::write(&file, bytes).unwrap();
        let (count, w, h) = super::draw_pages(&file, &pages).unwrap();
        // 2 inches at 300 dpi
        assert_eq!((count, w, h), (2, 600, 600));
        let middle =
            |n: u32| image::open(pages.join(format!("page-{n}.png"))).unwrap().to_rgba8().get_pixel(300, 300).0;
        let (first, second) = (middle(1), middle(2));
        assert!(first[0] > 240 && first[2] < 20, "{first:?}");
        assert!(second[2] > 240 && second[0] < 20, "{second:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
