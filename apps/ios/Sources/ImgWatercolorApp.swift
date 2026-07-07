import SwiftUI

@main
struct ImgWatercolorApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .preferredColorScheme(.dark) // デスクトップ版と揃えたダーク基調
        }
    }
}
