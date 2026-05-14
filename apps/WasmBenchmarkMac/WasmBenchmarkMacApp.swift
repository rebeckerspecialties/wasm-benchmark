import SwiftUI

@main
struct WasmBenchmarkMacApp: App {
    var body: some Scene {
        WindowGroup {
            BenchmarkContentView()
                .frame(minWidth: 480, minHeight: 320)
        }
    }
}
