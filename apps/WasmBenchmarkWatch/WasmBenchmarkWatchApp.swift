import SwiftUI

@main
struct WasmBenchmarkWatchApp: App {
    @State private var session = BenchmarkSession()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
        }
    }
}
