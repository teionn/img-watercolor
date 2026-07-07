# 算法说明：基于流场的笔触绘画渲染

> 日本語版: [algorithm.md](algorithm.md)

把照片转换成油画速涂风格的算法。属于 **Stroke-Based Rendering (SBR)** 家族
（多尺寸笔触的经典做法见 Hertzmann 1998，流场平滑见 Kang 等人的 Edge Tangent Flow），
设计出发点是模拟画家的实际作画流程：

- **沿面的方向运笔**——笔触方向来自整体平滑过的方向场，而不是逐点梯度；
- **暗色打底、亮色后上**——全部笔触按明度排序绘制，叠压关系和真实厚涂一致；
- **按画面密度换笔刷**——轮廓和狭窄处用小硬刷，开阔低反差处用大软刷；
- **主动删减信息**——低分辨率 + 少量色板不是性能妥协，而是刻意的"概括"，
  等价于画家眯眼观察。

## 一、管线总览

```
输入图
 ├─ 概括     长边缩到 pixels px → 双边滤波 → K-Means 减色到 palette 色
 │            ├─► 量化图（笔触取色来源）
 │            ├─► Posterize 色块边界
 │            └─► 色板格子 / 色环散点（色相→角度、饱和度→半径、点面积∝占比）
 ├─ 方向场   灰度 + 高斯模糊(normal_blur) → Sobel 梯度
 │            ├─► 法线图（把灰度当高度场，故整体偏蓝紫）
 │            └─► 结构张量平滑 → 流场 θ(x,y)（边缘切线方向）
 ├─ 密度图   归一化( 模糊(|梯度|) + 模糊(色块边界) )
 │            └─► 按分位数三档：前 15% 硬刷 / 55%~85% 标准刷 / 其余软刷
 │                密度越高笔刷越小
 └─ 绘制     底色 → 主层 → 细节 三个 Pass，全部笔触按明度由暗到亮逐笔盖章
              画完后从低分辨率画布三次插值放大到输出尺寸
```

## 二、阶段详解

### 概括与减色（palette.py）

- 长边缩到 `pixels` px——刻意取得很小，形状和颜色在这一步就被概括掉。
- K-Means 前先做双边滤波，否则毛发/纹理级噪声会让色块边界全是椒盐点；
  聚类标签再做一次 3×3 多数投票清残余斑点。
- 相邻像素标签不同处即 Posterize 边界——它既是可视化，也参与密度图，
  还通过"走线遇色突变即停"隐式约束笔触形状。

### 法线图与流场（flowmap.py）

- 笔触方向 = 边缘切线方向（等亮度线），是 **π 周期的"朝向"而非向量**。
  平滑和重采样必须在结构张量（等价于 2θ 复数域）上做，
  直接对角度求平均会在 0/π 交界处出错——这是流场顺滑的关键。
- 结构张量还给出相干度（各向异性程度），平坦区接近 0。
- 法线图：把模糊灰度当高度场，Sobel 梯度归一化后编码进 RGB。

### 密度图与笔刷调度（density.py）

- 密度 = 归一化( 模糊(梯度幅值) + 模糊(Posterize 边界) )，按分位数归一防极值压扁。
- 阈值按分位数自适应（硬刷=密度前 15%），固定阈值在毛发类高频图上会全图硬刷。
- 密度 → 笔刷半径：密度高 → 小；gamma < 1 让中等密度也偏向小刷子，轮廓才收得住。

### 笔触绘制（strokes.py + brushes.py）

- **走线**：种子点按抖动网格散布，沿流场双向走折线；每步把方向翻转到与上一步
  同侧（π 周期），遇目标颜色突变或出界即停。
- **渲染**：一笔 = 沿折线以固定间距盖"印章"（按局部走向旋转的灰度 alpha 贴图），
  两端做收笔渐隐（起笔/提笔压低不透明度，硬刷渐隐弱、软刷强）。
- **排序**：全部笔触按明度升序（暗→明）绘制——亮色永远压在暗色上，
  是"油画感"的最大来源之一。
- **湿混色**：落笔前与画布已有颜色按 `wet` 比例掺和，笔触间互相"带色"。
- **侧向采色**：部分笔触从垂直流向的偏移处采样颜色后混合，
  模仿画家从相邻色面"借色"的习惯。
- **三个 Pass**：底色（大软刷、超大间距）→ 主层（密度定尺寸/软硬）→
  细节（只在高密度区补小硬刷，不做湿混色，轮廓要咬得住）。
- 笔刷贴图程序化生成（triangle 三角刀 / flat 平头 / soft 软圆 / oil / pastel / charcoal），
  也可加载 Photoshop 笔尖导出的灰度 PNG。
- 调试视图 `7_strokes_debug.png` 把每根笔触按档位着色
  （红=硬刷、绿=标准刷、蓝=软刷），用于检查笔刷调度是否合理。

## 三、质感的三个关键

1. **低分辨率画布 + 平滑放大**：整幅画在长边 `resolution` px 的画布上画完，
   再三次插值放大。笔触边缘全是抗锯齿过渡——"像笔刷不像方块"的最大来源。
   在大图上直接画，任何贴图瑕疵都是像素级硬边。这个值越小成品越抽象
   （200 已经是很强的速涂感，360 保留更多细节）。
2. **全软边笔刷贴图**：alpha 一律平滑衰减，不用二值掩码；软刷带极淡的鬃毛纹理，
   大面积平涂才会留下顺流向的丝痕（纯高斯像喷枪）。
3. **湿混色 + 收笔渐隐**：颜色互相带染、笔触有起收，成品才有手绘的呼吸感。

## 四、参数速查

`painterly/pipeline.py` 的 `PARAMS_DEFAULT`，CLI/GUI 同名：

| 参数 | 默认 | 含义 |
|---|---|---|
| pixels | 96 | 取色/概括分辨率（长边 px），越小越概括 |
| resolution | 360 | 绘画画布长边 px，越小越抽象 |
| palette | 36 | 减色数（K-Means 的 K） |
| color_space | rgb | 量化色彩空间 rgb / lab |
| posterize_blur | 2.0 | 量化前高斯 σ |
| normal_blur | 8.0 | 求梯度前高斯 σ |
| brush_size | 15 | 基准笔刷半径（画布 px） |
| brush_length | 3.0 | 笔触长度上限（半径的倍数）；调低成点彩短笔，调高成流动长笔 |
| hard/standard/soft_brush | triangle/flat/soft | 三档笔刷贴图 |
| strokes_scale | 1.0 | 笔触密度倍率 |
| hard/standard_quantile | 0.85 / 0.55 | 笔刷档位的密度分位数阈值 |
| wet | 0.18 | 湿混色比例 |
| saturation | 1.15 | 取色饱和度增益 |
| out_long | 1080 | 输出长边（放大后） |

## 五、代码对照

```
painterly/
  palette.py   概括与减色、Posterize 边界、色板/色环可视化
  flowmap.py   法线图、结构张量流场
  density.py   密度图、笔刷尺寸/档位调度
  brushes.py   笔刷贴图（程序化 + 自定义 PNG）
  strokes.py   走线、排序、湿混色、盖章渲染、调试视图
  pipeline.py  编排 + 中间图落盘 + GUI 回调钩子
run.py         命令行入口
gui.py         PySide6 图形界面
```

## 六、参考文献

- A. Hertzmann, *Painterly Rendering with Curved Brush Strokes of Multiple Sizes*, SIGGRAPH 1998
- H. Kang, S. Lee, C. Chui, *Coherent Line Drawing*, NPAR 2007（Edge Tangent Flow）
- J. E. Kyprianidis, J. Döllner, *Image Abstraction by Structure Adaptive Filtering*, 2008
- A. Semmo et al., *Oil Paint Filtering Using Color Palettes*, 2016
- 综述：Kyprianidis et al., *State of the "Art": A Taxonomy of Artistic Stylization Techniques*, TVCG 2013
