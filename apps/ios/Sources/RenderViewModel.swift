import Photos
import SwiftUI

/// 中間ステージ 1 枚（表示順は ContentView.stageOrder に従う）
struct Stage: Identifiable, Equatable {
    let id: String
    let label: String
    let image: UIImage
}

/// ステージ名 → 日本語ラベル（デスクトップ版と同一）
let stageLabels: [String: String] = [
    "1_original": "元画像",
    "2_quantized": "減色",
    "3_posterize_edges": "境界線",
    "4_gray_blur": "グレー",
    "5_normal_map": "法線",
    "6_flow_map": "フロー",
    "density": "密度",
    "palette_swatch": "パレット",
    "color_wheel": "色環",
    "7_strokes_debug": "ストローク",
    "8_painting": "完成",
]
let stageOrder: [String] = [
    "1_original", "2_quantized", "3_posterize_edges", "4_gray_blur",
    "5_normal_map", "6_flow_map", "density", "palette_swatch",
    "color_wheel", "7_strokes_debug", "8_painting",
]

@MainActor
final class RenderViewModel: ObservableObject {
    @Published var originalData: Data?
    @Published var preview: UIImage?
    @Published var stages: [Stage] = []
    @Published var progress: Double = 0
    @Published var isRendering = false
    @Published var statusText = "写真を選んでください"
    @Published var params = defaultParams()
    @Published var finalImage: UIImage?

    /// 描画過程のフレーム（リプレイ用）
    private(set) var frames: [UIImage] = []
    private var replayTask: Task<Void, Never>?

    var canRender: Bool { originalData != nil && !isRendering }
    var canReplay: Bool { !frames.isEmpty && !isRendering }

    func loadImage(_ data: Data) {
        stopReplay()
        originalData = data
        stages = []
        frames = []
        finalImage = nil
        progress = 0
        if let img = UIImage(data: data) {
            preview = img
            statusText = "\(Int(img.size.width))×\(Int(img.size.height))"
        } else {
            preview = nil
            statusText = "画像を読み込めません"
        }
    }

    func startRender() {
        guard let data = originalData, !isRendering else { return }
        stopReplay()
        isRendering = true
        progress = 0
        frames = []
        stages = []
        finalImage = nil
        statusText = "レンダリング中…"
        let params = self.params
        let observer = ObserverBridge(viewModel: self)
        Task.detached(priority: .userInitiated) { [weak self] in
            do {
                // render はブロッキングなのでバックグラウンドタスクで呼ぶ
                let result = try render(imageBytes: data, params: params, observer: observer)
                await MainActor.run { self?.finishRender(result) }
            } catch {
                await MainActor.run { self?.failRender(error) }
            }
        }
    }

    func addStage(name: String, png: Data) {
        guard let img = UIImage(data: png) else { return }
        let stage = Stage(id: name, label: stageLabels[name] ?? name, image: img)
        if let idx = stages.firstIndex(where: { $0.id == name }) {
            stages[idx] = stage
        } else {
            // stageOrder の定義順を保って挿入
            let pos = stages.firstIndex {
                (stageOrder.firstIndex(of: $0.id) ?? .max) > (stageOrder.firstIndex(of: name) ?? .max)
            }
            stages.insert(stage, at: pos ?? stages.count)
        }
    }

    func addProgress(frac: Float, png: Data) {
        guard let img = UIImage(data: png) else { return }
        progress = Double(frac)
        frames.append(img)
        preview = img // 描画の進行をリアルタイム表示
    }

    private func finishRender(_ result: RenderResult) {
        isRendering = false
        progress = 1
        if let img = UIImage(data: result.png) {
            finalImage = img
            preview = img
        }
        statusText = "完成: \(result.strokes) ストローク, \(String(format: "%.1f", Double(result.millis) / 1000)) 秒"
    }

    private func failRender(_ error: Error) {
        isRendering = false
        statusText = "エラー: \(error.localizedDescription)"
    }

    func replayProcess() {
        guard !frames.isEmpty else { return }
        stopReplay()
        let frames = self.frames
        replayTask = Task { [weak self] in
            for frame in frames {
                if Task.isCancelled { return }
                self?.preview = frame
                try? await Task.sleep(nanoseconds: 40_000_000)
            }
            if let final = self?.finalImage {
                self?.preview = final
            }
        }
    }

    func stopReplay() {
        replayTask?.cancel()
        replayTask = nil
    }

    func showStage(_ stage: Stage) {
        stopReplay()
        preview = stage.image
    }

    func saveToPhotos() {
        guard let img = finalImage else { return }
        PHPhotoLibrary.requestAuthorization(for: .addOnly) { [weak self] status in
            guard status == .authorized || status == .limited else {
                Task { @MainActor in self?.statusText = "写真ライブラリへのアクセスが許可されていません" }
                return
            }
            PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAsset(from: img)
            } completionHandler: { ok, error in
                Task { @MainActor in
                    self?.statusText = ok ? "写真に保存しました" : "保存失敗: \(error?.localizedDescription ?? "")"
                }
            }
        }
    }
}

/// Rust のレンダリングスレッドから届くコールバックを MainActor へ橋渡しする
private final class ObserverBridge: RenderObserver, @unchecked Sendable {
    weak var viewModel: RenderViewModel?

    init(viewModel: RenderViewModel) {
        self.viewModel = viewModel
    }

    func onStage(name: String, png: Data) {
        Task { @MainActor [weak viewModel] in
            viewModel?.addStage(name: name, png: png)
        }
    }

    func onProgress(frac: Float, png: Data) {
        Task { @MainActor [weak viewModel] in
            viewModel?.addProgress(frac: frac, png: png)
        }
    }
}
