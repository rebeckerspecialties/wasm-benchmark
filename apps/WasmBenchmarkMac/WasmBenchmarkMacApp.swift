import SwiftUI

@main
struct WasmBenchmarkMacApp: App {
    @State private var session = BenchmarkSession()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(session)
                .frame(minWidth: 720, minHeight: 480)
        }
    }
}
