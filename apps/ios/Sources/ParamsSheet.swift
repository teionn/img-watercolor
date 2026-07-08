import SwiftUI

/// レンダリングパラメータの編集シート（デスクトップ版サイドパネルと同一項目）
struct ParamsSheet: View {
    @Binding var params: RenderParams
    @Environment(\.dismiss) private var dismiss

    private let brushes = builtinBrushes()

    var body: some View {
        NavigationStack {
            Form {
                Section("解像度") {
                    intSlider("採色解像度", value: $params.pixels, range: 32...256, step: 8, unit: "px")
                    intSlider("キャンバス解像度", value: $params.resolution, range: 120...720, step: 20, unit: "px")
                    intSlider("出力長辺", value: $params.outLong, range: 480...2160, step: 40, unit: "px")
                }
                Section("色") {
                    intSlider("減色数", value: $params.palette, range: 8...96, step: 2, unit: "")
                    floatSlider("減色前ぼかし σ", value: $params.posterizeBlur, range: 0...6, step: 0.5)
                    floatSlider("彩度", value: $params.saturation, range: 0.8...1.6, step: 0.05)
                    Picker("色空間", selection: $params.colorSpace) {
                        Text("RGB").tag("rgb")
                        Text("Lab").tag("lab")
                    }
                }
                Section("ストローク") {
                    floatSlider("ブラシ半径", value: $params.brushSize, range: 4...40, step: 1)
                    floatSlider("ストローク密度", value: $params.strokesScale, range: 0.5...2, step: 0.1)
                    floatSlider("ウェット混色", value: $params.wet, range: 0...0.5, step: 0.02)
                    floatSlider("勾配ぼかし σ", value: $params.normalBlur, range: 1...20, step: 0.5)
                }
                Section("奥行き") {
                    floatSlider("奥行きディテール", value: $params.depthDetail, range: 0...1, step: 0.05)
                    Toggle("手前/奥を反転", isOn: $params.depthInvert)
                }
                Section("ブラシ") {
                    brushPicker("ハード", selection: $params.hardBrush)
                    brushPicker("スタンダード", selection: $params.standardBrush)
                    brushPicker("ソフト", selection: $params.softBrush)
                }
                Section {
                    Button("既定値に戻す", role: .destructive) {
                        params = defaultParams()
                    }
                }
            }
            .navigationTitle("パラメータ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("完了") { dismiss() }
                }
            }
        }
    }

    private func brushPicker(_ label: String, selection: Binding<String>) -> some View {
        Picker(label, selection: selection) {
            ForEach(brushes, id: \.self) { Text($0).tag($0) }
        }
    }

    private func intSlider(
        _ label: String, value: Binding<UInt32>,
        range: ClosedRange<Double>, step: Double, unit: String
    ) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(label)
                Spacer()
                Text("\(value.wrappedValue)\(unit)")
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            Slider(
                value: Binding(
                    get: { Double(value.wrappedValue) },
                    set: { value.wrappedValue = UInt32($0.rounded()) }
                ),
                in: range, step: step
            )
        }
    }

    private func floatSlider(
        _ label: String, value: Binding<Float>,
        range: ClosedRange<Double>, step: Double
    ) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(label)
                Spacer()
                Text(String(format: "%.2f", value.wrappedValue))
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            Slider(
                value: Binding(
                    get: { Double(value.wrappedValue) },
                    set: { value.wrappedValue = Float($0) }
                ),
                in: range, step: step
            )
        }
    }
}
