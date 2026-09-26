import SwiftUI

@main
struct WasmBenchmarkVisionApp: App {
    @State private var session = BenchmarkSession()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
        }
        .defaultSize(width: 1180, height: 820)
    }
}
