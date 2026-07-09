# 水彩レンダリング研究サーベイ（2026-07 調査）

写真 → 水彩画変換に関する学術文献の調査。手法は大きく 4 系統に分かれる。
本プロジェクト（ストロークベース + 後処理）にどう取り込むかの評価付き。

---

## 系統 1: 物理シミュレーション（流体 + 顔料光学）

### Curtis et al., *Computer-Generated Watercolor*（SIGGRAPH 1997）★基礎文献

- [論文 PDF](https://www.cs.princeton.edu/courses/archive/fall00/cs597b/papers/curtis97.pdf) /
  [プロジェクトページ](https://grail.cs.washington.edu/projects/watercolor/)
- 水彩の見た目を初めて体系的に定式化。**浅水（shallow-water）流体シミュレーション**を
  3 層（水層・顔料沈着層・毛細管層）で行い、光学は **Kubelka-Munk（K-M）モデル**で
  透明グレーズを重ねる。
- 再現される効果: **エッジ暗色化・粒状化・バックラン（乾き際の花咲き）・
  顔料分離・グレージング**。以後の全研究がこの効果リストを参照する。
- 欠点: 遅い（当時で数分/枚）。写真からの自動変換には別途色分解が必要。

### Chu & Tai, *MoXi: Real-time Ink Dispersion in Absorbent Paper*（SIGGRAPH 2005）

- [ACM](https://dl.acm.org/doi/10.1145/1073204.1073221)
- **格子ボルツマン法**で紙への浸透・にじみを GPU 実時間化。東洋の水墨が対象だが、
  [西洋水彩への拡張レポート](https://www.semanticscholar.org/paper/4c15beebfc6aad0bac01fe0ae8a5385c682f6ab9)もある。
  にじみ（wet-in-wet）の品質は今でも最高峰。
- Van Laerhoven et al.（2004-05）も分散紙モデルでの実時間水彩を提案。

**本プロジェクトへの適用**: フル流体は iOS 実時間には過剰。ただし「バックラン」
「wet-in-wet の局所拡散」だけなら、密度場に対する単純な異方性拡散
（数十イテレーションのぼかし + 流向バイアス）で近似する余地がある。

---

## 系統 2: フィルタベース NPR（シミュレーションなしの見た目再現）★本プロジェクトに最も近い

### Bousseau et al., *Interactive Watercolor Rendering with Temporal Coherence and Abstraction*（NPAR 2006）

- [ResearchGate](https://www.researchgate.net/publication/221523186_SILLION_F_Interactive_watercolor_rendering_with_temporal_coherence_and_abstraction)
- 水彩効果を**すべてテクスチャと画像処理の後処理**で再現する古典。
  - 抽象化: モルフォロジー平滑化で色面を単純化（うちの減色 + 低解像度化に相当）
  - **顔料密度の変調**: `C' = C(1 + (1-C)(d-1))` — 密度 d を
    紙目・乱流ノイズ（Perlin）でゆらす。乗算より発色が良い黒板公式
  - エッジ暗色化・**輪郭のゆらぎ（wobble）**: ノイズでサンプリング座標を歪める
- **教訓**: 水彩の「らしさ」の大半は 紙目 × 低周波乱流 × エッジ暗色化 ×
  輪郭ゆらぎ の 4 つの後処理で出る。

### Montesdeoca et al., *Art-directed Watercolor Stylization of 3D Animations in Real-time*（Computers & Graphics 2017）/ *MNPR*（Expressive 2018）

- [C&G 論文](https://www.sciencedirect.com/science/article/abs/pii/S0097849317300316) /
  [MNPR（OSS, Maya 実装）](https://www.researchgate.net/publication/325796023_MNPR_A_Framework_for_Real-Time_Expressive_Non-Photorealistic_Rendering_of_3D_Computer_Graphics) /
  [解説ページ](https://artineering.io/styles/watercolor)
- 3D 向けだが効果の実装が具体的で移植しやすい:
  - **エッジ暗色化 = Difference-of-Gaussians による特徴強調**（勾配ベース。
    うちのスタンプ縁リム関数より画像全体で均質に効く）
  - **手ぶれ（hand tremor）**: 輪郭に沿った高周波の座標ゆらぎ
  - **pigment turbulence**: 低周波ノイズで色面内の濃度をむらす
  - **color bleeding**: オブジェクト/画像空間ハイブリッドのぼかし
- [Edge- and substrate-based effects for watercolor stylization](https://www.academia.edu/64725262/Edge_and_substrate_based_effects_for_watercolor_stylization)
  （Expressive 2017）はエッジ系効果だけを独立に詳述。

**本プロジェクトへの適用**（優先度高）:
1. **顔料密度変調式** `C(1+(1-C)(d-1))` を現在の紙目乗算・粒状化の代わりに採用
   → 明部で紙が透け、暗部で沈む正しい挙動
2. **エッジ暗色化を画像空間 DoG でも**掛ける（ストローク縁リムと相補的）
3. **輪郭ゆらぎ（wobble）**: 減色ターゲットのサンプリング座標を
   低周波ノイズで ±2〜3px 歪める → 「手描きの線の揺れ」が全段に波及する

---

## 系統 3: 写真 → 水彩の自動変換（本プロジェクトと同じ課題設定）

### Wang et al., *Towards Photo Watercolorization with Artistic Verisimilitude*（IEEE TVCG 2014）

- [論文 PDF](http://www.cs.columbia.edu/cg/raymond/watercolor/watercolor_pp.pdf) /
  [PubMed](https://pubmed.ncbi.nlm.nih.gov/26357391/)
- 写真からの水彩化で最も引用される研究。ユーザースタディで
  フィルタ系・物理系の従来法より高評価。
- 構成要素:
  - **水彩らしい色への転写**（実際の水彩作品からの色統計マッチング）
  - **顕著性（saliency）ベースの詳細度制御** — 主題は描き込み、背景は省略
    （うちの「深度フォーカス」と同じ思想。顕著性 × 深度の併用が示唆される）
  - **手ぶれ効果**（神経ノイズ由来の輪郭ゆらぎ）
  - **wet-in-wet の境界にじみ**を湿り気の異なる色面境界に選択的に発生させる
- DiVerdi et al., *A Lightweight, Procedural, Vector Watercolor Painting Engine*
  （[I3D 2013](https://dl.acm.org/doi/10.1145/2159616.2159627)、Adobe）:
  ストローク単位の手続き的水彩（スプラット粒子で境界を進化）。
  ベクタで解像度非依存。「ストロークベースで水彩」という点でうちの構成に近い。

**本プロジェクトへの適用**:
- 色転写（水彩パレットへの寄せ）はパレット量子化の直後に統計マッチングを
  1 段挟むだけで導入できる
- 顕著性マップは深度と同様に密度変調へ合成可能（将来: 顕著性モデルを
  painterly-depth と同じ tract 経路で追加）

---

## 系統 4: 顔料光学（色の混ざり方）

### Sochorová & Jamriška, *Practical Pigment Mixing for Digital Painting*（Mixbox, SIGGRAPH Asia 2021）

- [プロジェクト](https://dcgi.fel.cvut.cz/en/publications/2021/sochorova-tog-pigments/) /
  [論文 PDF](https://dcgi.fel.cvut.cz/wp-content/wpallimport-dist/publications/pdf/publications-2021-sochorova-tog-pigments-paper.pdf)
- RGB の線形補間は「光の混色」なので 青 + 黄 = 灰 になる。Mixbox は内部で
  K-M の顔料表現に持ち上げてから混ぜ、**青 + 黄 = 緑** を RGB API のまま実現。
- ライセンス注意: 参照実装は **CC BY-NC 4.0（非商用）**。製品化する場合は
  ライセンス契約か、論文の手法を独自実装する必要がある（アルゴリズム自体は公開）。

**本プロジェクトへの適用**: ウェットブレンディングと darken-only グレーズの
線形 RGB 混色を K-M 系に置き換えると、重色・にじみの発色が本物の絵の具に近づく。
効果が大きいのは「補色同士の混色が濁る」挙動。まず簡易 K-M
（単色顔料の K/S 近似）で自前実装するのが現実的。

---

## 系統 5: 学習ベース（参考）

- スタイル転送 / 拡散モデル系（[サーベイ](https://arxiv.org/pdf/2408.12128)、
  [Awesome リスト](https://github.com/Westlake-AGI-Lab/Awesome-Style-Transfer-with-Diffusion-Models)）は
  見た目の再現力は高いが、①パラメータの意味的な制御が難しい、②モバイル実行が重い、
  ③構図の忠実性が保証されない、ため本プロジェクトの方針（解釈可能な
  パラメータでアートディレクション可能）とは相性が悪い。
- 採用するなら Depth Anything と同様「補助マップの推定」
  （顕著性・セグメンテーション・法線）に限定するのが良い。

---

## まとめ: 実装ロードマップへの反映案

| 優先 | 施策 | 出典 | 実装コスト |
|---|---|---|---|
| ◎ | 顔料密度変調 `C(1+(1-C)(d-1))` へ置換 | Bousseau 2006 | 小（数行） |
| ◎ | 輪郭ゆらぎ（wobble）: ターゲットサンプリングの低周波歪み | Bousseau 2006 / MNPR | 小 |
| ○ | 画像空間 DoG によるエッジ暗色化の追加 | MNPR 2017 | 小 |
| ○ | wet-in-wet 境界にじみ（色面境界の選択的拡散） | Wang 2014 / Curtis 1997 | 中 |
| ○ | 水彩パレットへの色統計転写 | Wang 2014 | 中 |
| △ | K-M / Mixbox 系の顔料混色（ライセンス注意） | Sochorová 2021 | 中 |
| △ | 顕著性マップによる詳細度制御（深度と併用） | Wang 2014 | 中（モデル追加） |
| × | フル流体シミュレーション | Curtis 1997 / MoXi 2005 | 大・モバイル不向き |

現在の実装（透明グレーズ min / エッジリム / 粒状化 / 紙余白）は
Curtis の効果リストの静的近似として妥当な位置にいる。次の一手として
費用対効果が最も高いのは **Bousseau の密度変調式 + 輪郭ゆらぎ** の 2 点。
