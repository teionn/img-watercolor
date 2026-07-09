import PhotosUI
import SwiftUI

struct ContentView: View {
    @StateObject private var vm = RenderViewModel()
    @State private var pickerItem: PhotosPickerItem?
    @State private var showParams = false
    @State private var previewSize: CGSize = .zero

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                previewArea
                if !vm.stages.isEmpty {
                    stageStrip
                }
                controlBar
            }
            .navigationTitle("img-watercolor")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    PhotosPicker(selection: $pickerItem, matching: .images) {
                        Label("写真を選ぶ", systemImage: "photo.badge.plus")
                    }
                    .disabled(vm.isRendering)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showParams = true
                    } label: {
                        Label("パラメータ", systemImage: "slider.horizontal.3")
                    }
                    .disabled(vm.isRendering)
                }
            }
            .sheet(isPresented: $showParams) {
                ParamsSheet(params: $vm.params)
            }
            .onChange(of: pickerItem) { item in
                guard let item else { return }
                Task {
                    if let data = try? await item.loadTransferable(type: Data.self) {
                        vm.loadImage(data)
                    }
                }
            }
        }
    }

    private var previewArea: some View {
        ZStack {
            Color(.systemBackground)
            if let img = vm.preview {
                Image(uiImage: img)
                    .resizable()
                    .scaledToFit()
                    .background(
                        // scaledToFit 後のビューサイズ = 表示中の画像の実寸
                        GeometryReader { g in
                            Color.clear
                                .onAppear { previewSize = g.size }
                                .onChange(of: g.size) { previewSize = $0 }
                        }
                    )
                    .overlay(alignment: .topLeading) {
                        // フォーカスマーカー
                        if let fx = vm.params.focusX, let fy = vm.params.focusY,
                           previewSize != .zero {
                            Circle()
                                .stroke(Color.yellow, lineWidth: 2)
                                .shadow(radius: 1)
                                .frame(width: 18, height: 18)
                                .position(
                                    x: CGFloat(fx) * previewSize.width,
                                    y: CGFloat(fy) * previewSize.height
                                )
                        }
                    }
                    .onTapGesture(coordinateSpace: .local) { p in
                        // タップ位置の深度を焦点にする（正規化座標で渡す）
                        guard previewSize != .zero, !vm.isRendering else { return }
                        vm.params.focusX = Float(min(max(p.x / previewSize.width, 0), 1))
                        vm.params.focusY = Float(min(max(p.y / previewSize.height, 0), 1))
                    }
                    .padding(8)
            } else {
                ContentUnavailableViewCompat()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var stageStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                ForEach(vm.stages) { stage in
                    VStack(spacing: 2) {
                        Image(uiImage: stage.image)
                            .resizable()
                            .scaledToFill()
                            .frame(width: 76, height: 60)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                        Text(stage.label)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    .onTapGesture { vm.showStage(stage) }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .background(Color(.secondarySystemBackground))
    }

    private var controlBar: some View {
        VStack(spacing: 8) {
            ProgressView(value: vm.progress)
                .tint(.blue)
            HStack(spacing: 12) {
                Button {
                    vm.startRender()
                } label: {
                    Label("レンダリング", systemImage: "paintbrush.pointed.fill")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .disabled(!vm.canRender)

                Button {
                    vm.replayProcess()
                } label: {
                    Image(systemName: "play.circle")
                }
                .buttonStyle(.bordered)
                .disabled(!vm.canReplay)

                Button {
                    vm.saveToPhotos()
                } label: {
                    Image(systemName: "square.and.arrow.down")
                }
                .buttonStyle(.bordered)
                .disabled(vm.finalImage == nil)
            }
            Text(vm.statusText)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(12)
        .background(Color(.secondarySystemBackground))
    }
}

/// iOS 16 でも動く簡易プレースホルダ（ContentUnavailableView は iOS 17+）
private struct ContentUnavailableViewCompat: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "photo.on.rectangle.angled")
                .font(.system(size: 44))
                .foregroundStyle(.tertiary)
            Text("左上の「写真を選ぶ」から始めてください")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}
