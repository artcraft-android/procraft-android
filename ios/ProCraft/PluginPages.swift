import SwiftUI

/// The tag on a card.
private var pluginTag: Color { Look.shared.light ? Color(argb: 0x1400_0000) : Color(argb: 0x1FFF_FFFF) }

/// The patches, or the plugins, a page of the panel: a card each, drawn from what the list says of
/// them.
struct PluginsPage: View {
    let model: CanvasModel
    let patches: Bool
    let onBack: () -> Void

    @State private var plugins = PluginsModel.shared
    @State private var importing = false

    var body: some View {
        let shown = patches ? plugins.patches : plugins.plugins
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                PageHeader(title: patches ? "Patches" : "Plugins", onBack: onBack)
                Spacer(minLength: 0)
                Text(verbatim: "Import")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
                    .padding(.horizontal, 16)
                    .frame(height: 34)
                    .contentShape(Rectangle())
                    .onTapGesture { importing = true }
                    .padding(.top, 12)
            }
            ScrollView {
                VStack(spacing: 20) {
                    if shown.isEmpty {
                        Text(verbatim: "Nothing imported yet. Import a .procraftplugin file to add one.")
                            .font(.system(size: 15))
                            .foregroundStyle(Pc.actionsRow)
                            .padding(16)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    ForEach(shown, id: \.id) { plugin in
                        PluginCard(model: model, plugin: plugin)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 13)
            }
            .scrollIndicators(.hidden)
            .frame(maxHeight: 760)
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item]) { result in
            if case let .success(url) = result {
                plugins.offer(url) { model.showNotice($0, warning: true) }
            }
        }
        .onAppear { plugins.read() }
    }
}

/// A patch or plugin: its name and tag, what it does, what is the matter with it, its controls
/// whatever they are, and the row that applies or removes it.
private struct PluginCard: View {
    let model: CanvasModel
    let plugin: PluginInfo

    private var line: some View {
        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
    }

    private var about: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Text(verbatim: plugin.name)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                if plugin.experimental {
                    Text(verbatim: "experimental")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(Pc.actionsRow)
                        .lineLimit(1)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 2)
                        .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(pluginTag))
                }
            }
            Text(verbatim: plugin.description)
                .font(.system(size: 14))
                .foregroundStyle(Pc.actionsRow)
                .padding(.top, 6)
            if let problem = plugin.problem {
                Text(verbatim: problem)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Pc.slideValue)
                    .padding(.top, 4)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func control(_ control: PluginControl) -> some View {
        switch control {
        case let .switch(one):
            RowView(row: .toggle(one.label, on: one.on, change: { PluginsModel.shared.setSwitch(one.key, $0) }))
        case let .slider(one):
            PluginSliderRow(slider: one) { PluginsModel.shared.setSlider($0, $1) }
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 22, style: .circular)
        let plugins = PluginsModel.shared
        VStack(spacing: 0) {
            about
            // an extra's always, a patch's once applied; a filter's are on its own panel
            if plugin.on, plugin.problem == nil, plugin.kind != .filter {
                ForEach(plugin.controls.indices, id: \.self) { i in
                    line
                    control(plugin.controls[i])
                }
            }
            if plugin.kind == .patch {
                line
                RowView(row: .act(plugin.on ? "Unapply" : "Apply", action: { plugins.setOn(plugin, !plugin.on) }))
            }
            // a filter that stopped stays off until someone asks
            if plugin.kind == .filter, !plugin.on {
                line
                RowView(row: .act("Turn on", action: { plugins.setOn(plugin, true) }))
            }
            if plugin.source == .file {
                line
                RowView(row: .act("Remove", action: {
                    if !plugins.remove(plugin) {
                        model.showNotice("This plugin couldn't be removed.", warning: true)
                    }
                }))
            }
        }
        .background(shape.fill(Pc.actionsGroup))
        .clipShape(shape)
    }
}

/// A slider on a card: the row follows the finger, and the value goes in as it lets go, as what it
/// sets may move the row itself, unless it is live.
private struct PluginSliderRow: View {
    let slider: PluginSlider
    let onSet: (PluginSlider, Float) -> Void

    @State private var moved: Float?

    var body: some View {
        let value = moved ?? slider.value
        ActionSlideRow(
            label: slider.label,
            value: pluginSliderShown(slider: slider, value: value),
            share: pluginSliderAlong(slider: slider, value: value),
            change: { along in
                let to = pluginSliderAt(slider: slider, along: along)
                if slider.live {
                    onSet(slider, to)
                } else {
                    moved = to
                }
            },
            onDone: {
                if let moved {
                    onSet(slider, moved)
                }
                moved = nil
            }
        )
    }
}

/// The one screen that asks whether to import a plugin file: what it is, and what it can do. Also
/// where a setting that has moved to a patch is said, once.
struct PluginAlerts: View {
    @State private var plugins = PluginsModel.shared
    /// What went wrong keeping a file.
    @State private var failed: String?

    var body: some View {
        ZStack {
            AlertView(
                content: plugins.movedNotice.map {
                    AlertContent(title: "Now a patch", message: $0, action: "OK", destructive: false, alone: true)
                },
                onCancel: { plugins.movedNoticeRead() },
                onAction: { plugins.movedNoticeRead() }
            )
            AlertView(
                content: plugins.offer.map {
                    AlertContent(title: $0.name, message: $0.message, action: "Import", destructive: false)
                },
                onCancel: { plugins.declineOffer() },
                onAction: { failed = plugins.acceptOffer() }
            )
            AlertView(
                content: failed.map {
                    AlertContent(title: "Can't import this file", message: $0, action: "OK", destructive: false, alone: true)
                },
                onCancel: { failed = nil },
                onAction: { failed = nil }
            )
        }
    }
}

extension CanvasModel {
    /// The session goes by the plugins that reach a canvas.
    func applyPlugins() {
        let plugins = PluginsModel.shared.store
        session.usePatches(plugins: plugins)
        session.setFillGap(pixels: plugins.fillGap())
        session.setInterfaceSize(size: Float(Interface.shared.scale))
    }

    func setPluginFilterSlider(_ key: String, _ value: Float) {
        _ = session.pluginFilterSlider(key: key, value: value, done: false)
        readAdjust()
    }

    /// The finger left the slider `key`: a filter too slow to follow it redraws now.
    func letGoPluginFilter(_ key: String) {
        for control in adjustShown?.plugin?.controls ?? [] {
            if case let .slider(slider) = control, slider.key == key {
                _ = session.pluginFilterSlider(key: key, value: slider.value, done: true)
            }
        }
    }

    func setPluginFilterSwitch(_ key: String, _ on: Bool) {
        session.pluginFilterSwitch(key: key, on: on)
        readAdjust()
    }

    /// A filter that stopped was closed by the core, which says why.
    func checkPluginFilter() {
        guard adjustment == .plugin, session.adjustmentShown().adjustment == nil else { return }
        if let failure = session.takeFailure() {
            showNotice(failure, warning: true)
        }
        adjustMenuAt = nil
        readAdjust()
        refresh()
    }
}
