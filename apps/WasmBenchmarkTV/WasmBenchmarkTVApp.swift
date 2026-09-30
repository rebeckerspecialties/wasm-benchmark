import SwiftUI

@main
struct WasmBenchmarkTVApp: App {
    @State private var session = BenchmarkSession()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
        }
    }
}
