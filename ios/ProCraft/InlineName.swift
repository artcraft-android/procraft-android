import SwiftUI
import UIKit

/// A name edited where it is shown: focused with the keyboard up; done on the keyboard's return or
/// when focus leaves, once.
struct InlineName: View {
    let name: String
    let color: Color
    let fontSize: CGFloat
    let onDone: (String) -> Void

    @State private var value = ""
    @State private var finished = false
    @FocusState private var focused: Bool

    var body: some View {
        TextField(text: $value) { EmptyView() }
            .font(.system(size: fontSize))
            .foregroundStyle(color)
            .tint(color)
            .autocorrectionDisabled()
            .submitLabel(.done)
            .focused($focused)
            .onSubmit(finish)
            .onChange(of: focused) { was, now in
                if was, !now {
                    finish()
                }
            }
            .onAppear {
                value = name
                focused = true
                // the whole name chosen, so typing replaces it
                DispatchQueue.main.async {
                    UIApplication.shared.sendAction(#selector(UIResponder.selectAll(_:)), to: nil, from: nil, for: nil)
                }
            }
    }

    private func finish() {
        if !finished {
            finished = true
            onDone(value)
        }
    }
}
