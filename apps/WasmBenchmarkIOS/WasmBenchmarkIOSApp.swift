import SwiftUI

@main
struct WasmBenchmarkIOSApp: App {
    @State private var session = BenchmarkSession()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
        }
    }
}
