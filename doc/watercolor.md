# 水彩表現（Waterlogue を参考にした画像処理）

Waterlogue（Tinrocket）の見た目の核は、Curtis et al.
*Computer-Generated Watercolor*（SIGGRAPH 1997）が定式化した水彩の
光学的・物理的特徴の近似にある。本プロジェクトでは流体シミュレーションは行わず、
ストロークレンダリングに以下の 3 要素を組み込んで同系統の見た目を得る
（`crates/painterly-core/src/strokes.rs` の `WatercolorCfg`）。

## 1. 透明顔料のグレーズ（`pigment` 0..1）

水彩は不透明な上塗りではなく、紙の白が透ける減法混色。
実装は不透明アルファ合成と **darken-only グレーズ** のブレンド:

```
glaze  = min(下地色, 顔料色)          # 冪等: 同色の重ね塗りで沈まない
paint  = 顔料色 × (1−pigment) + glaze × pigment
下地   = 下地 × (1−α) + paint × α
```

単純な乗算（`下地 × 顔料色`）だと数千ストロークの重なりで色が際限なく沈む。
min によるグレーズは「明るくはならないが、同じ色で暗くもならない」という
水彩の性質（白は絵の具ではなく紙）を保ちながら発色が安定する。
`pigment > 0` のときは下塗りキャンバスも平均色 → 紙白へ連動して寄る。

## 2. エッジ暗色化（`edge_darken` 0..1）

乾く過程で顔料が塗りの縁に運ばれて溜まる、水彩で最も特徴的な縁取り。
スタンプ alpha の中間帯をリム関数 `4s(1−s)`（s=0.5 で最大）で増幅する:

```
α ← α × (1 + edge_darken × 4s(1−s))
```

ソフトエッジのブラシテクスチャと組み合わさり、塗りの外周だけが濃くなる。

## 3. 粒状化（granulation）

顔料が紙の目の谷に沈む効果。`pigment > 0` のとき `paper_texture` の強さで
ストローク alpha を紙目ノイズで変調する（シード固定）。
仕上げの紙テクスチャ（`paper.rs`）と同系統のノイズなので質感が揃う。

## 紙の余白（`paper_border`）

Waterlogue の額装感。最終出力の外周を低周波ノイズで荒らした
オフホワイトの紙余白でマスクする（`paper.rs`）。

## 使い方

```bash
painterly input/cat.png --preset 水彩
painterly input/cat.png --pigment 0.85 --edge-darken 0.65 --paper-border 0.05
```

GUI ではプリセット「水彩」を選ぶか、「透明水彩 / エッジ濃縮 / 紙の質感 / 紙の余白」
スライダーで個別調整する。

## 参考文献

- C. Curtis, S. Anderson, J. Seims, K. Fleischer, D. Salesin.
  *Computer-Generated Watercolor.* SIGGRAPH 1997.
- A. Hertzmann. *Painterly Rendering with Curved Brush Strokes of
  Multiple Sizes.* SIGGRAPH 1998.（ストローク基盤の方は従来どおり）
