//! 名前付きプリセット（Waterlogue 参考）。
//! パラメータの束に名前を付け、UI では 1 タップ / CLI では --preset で適用する。
//! 適用後に個別パラメータを上書きして微調整できる。

use crate::pipeline::Params;

pub struct Preset {
    pub name: &'static str,
    /// UI のツールチップ用の一言説明
    pub description: &'static str,
    pub params: Params,
}

/// 全プリセット。先頭が既定（基本）
pub fn presets() -> Vec<Preset> {
    let base = Params::default;
    vec![
        Preset {
            name: "基本",
            description: "バランスの取れた油彩スケッチ（既定値）",
            params: base(),
        },
        Preset {
            name: "水彩",
            description: "透明顔料のグレーズ + エッジ暗色化 + 粒状化。Waterlogue 風",
            params: Params {
                pigment: 0.85,
                edge_darken: 0.65,
                wet: 0.28,
                saturation: 1.3,
                paper_texture: 0.6,
                line_strength: 0.25,
                brush_size: 17.0,
                // 水彩専用ブラシ: 大きな面は wash、置き染みは bleed、
                // 細部はかすれ筆でドライブラシの質感
                hard_brush: "drybrush".into(),
                standard_brush: "bleed".into(),
                soft_brush: "wash".into(),
                ..base()
            },
        },
        Preset {
            name: "厚塗り",
            description: "深い剛毛の溝と絵具の塊。インパスト油彩",
            params: Params {
                brush_size: 16.0,
                wet: 0.22,
                saturation: 1.2,
                paper_texture: 0.2,
                line_strength: 0.2,
                hard_brush: "triangle".into(),
                standard_brush: "impasto".into(),
                soft_brush: "oil".into(),
                ..base()
            },
        },
        Preset {
            name: "鮮やか",
            description: "彩度を上げ、色数も少し多く",
            params: Params {
                saturation: 1.45,
                palette: 44,
                ..base()
            },
        },
        Preset {
            name: "淡彩",
            description: "薄い色と柔らかいタッチ、控えめな線",
            params: Params {
                saturation: 0.92,
                wet: 0.3,
                brush_size: 18.0,
                detail_min: 0.3,
                line_strength: 0.25,
                paper_texture: 0.55,
                hard_brush: "bleed".into(),
                standard_brush: "wash".into(),
                soft_brush: "wash".into(),
                ..base()
            },
        },
        Preset {
            name: "ポートレート",
            description: "人物向け。顔検出強化 + 高忠実の色境界 + 細部保持で目鼻を残す",
            params: Params {
                pixels: 260,
                resolution: 640,
                palette: 72,
                brush_size: 10.0,
                wet: 0.08,
                saturation: 1.05,
                color_fidelity: 0.75,
                detail_retention: 0.7,
                detail_overlay: 0.35,
                face_detail: 0.85,
                line_strength: 0.3,
                ..base()
            },
        },
        Preset {
            name: "細密",
            description: "解像度と色数を上げてディテールを残す",
            params: Params {
                pixels: 200,
                resolution: 560,
                palette: 64,
                brush_size: 11.0,
                strokes_scale: 1.2,
                ..base()
            },
        },
        Preset {
            name: "ラフ",
            description: "強い単純化と大きなタッチ、速描きの印象",
            params: Params {
                pixels: 72,
                resolution: 220,
                brush_size: 16.0,
                line_strength: 0.3,
                hard_brush: "drybrush".into(),
                ..base()
            },
        },
        Preset {
            name: "鉛筆画",
            description: "淡い色置きの上に強い鉛筆線。下書きの雰囲気",
            params: Params {
                palette: 16,
                saturation: 0.35,
                wet: 0.05,
                line_strength: 0.95,
                line_width: 1.4,
                paper_texture: 0.65,
                detail_min: 0.4,
                hard_brush: "pencil".into(),
                standard_brush: "pencil".into(),
                soft_brush: "pastel".into(),
                ..base()
            },
        },
    ]
}

/// 名前でプリセットを取得
pub fn preset(name: &str) -> Option<Params> {
    presets().into_iter().find(|p| p.name == name).map(|p| p.params)
}
