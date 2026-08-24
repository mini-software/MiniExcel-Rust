# MiniExcel .NET 功能缺口分析

[English](dotnet-feature-gaps.md)

## 比对范围

本文比对以下本地检出的公开可观察能力。它是一份特定时间点的实现待办清单，并不表示 Rust 应逐项照搬所有 .NET API。

| 项目 | 版本基线 |
| --- | --- |
| MiniExcel-Rust | `38ce3b220fb3a6ac06dec1026bacc0561d7cc94e`（`0.2.0`） |
| MiniExcel .NET | `5beb8b6986e93213af0b7ad8f0f1f6351b505d7e`（`2.0.0-preview.4-23-g5beb8b6`） |

比对依据包括同级目录 `../MiniExcel` 中的 .NET 公开 API、控制实现和聚焦测试，以及 Rust 的公开 `MiniExcel` 门面、选项、集成测试和[兼容性边界](compatibility.zh-CN.md)。

状态定义：

- **部分实现**：Rust 已覆盖核心场景，但尚未覆盖 .NET 的完整可观察行为。
- **未实现**：Rust 当前没有对应的公开能力。
- **设计不同**：Rust 提供了原生替代方案，但不构成 API 或行为等价。

## 已实现基线

Rust 已支持动态及 Serde 强类型 XLSX 路径查询、闭区间 A1 范围、工作表选择、表头与空行选项、工作表名称/信息/尺寸、有界内存行迭代、结构化单元格来源信息、面向浏览器的字节读取，以及新建单工作表或多工作表 XLSX。除非 .NET 的对应能力明显更广，下文不把这些能力列为缺失。

## 缺口矩阵

| 领域 | 状态 | .NET 已有而 Rust 尚未完整实现的能力 |
| --- | --- | --- |
| 命名表格 | 未实现 | 按表格名称查询 OpenXML Table（`QueryTable`），并遵循表格自身的表头与范围。 |
| DataReader 与 DataTable | 未实现 | `IDataReader`/异步 reader、架构表、强类型 getter、通过 `NextResult` 遍历工作表，以及物化为 `DataTable`。 |
| 调用方提供的流 | 部分实现 | 已实现借用的同步动态/类型化/structured visitor、metadata 读取，以及动态/schema/类型化/多表 writer，并保持 leave-open。借用 lazy iterator、async stream 和 template stream 仍不支持。 |
| 异步与取消 | 未实现 | 异步查询/导出/模板操作、异步行源、取消令牌和进度回调。Rust 路径读取虽使用工作线程，对外迭代器仍是同步的。 |
| 通用保存输入 | 部分实现 | 从普通对象/可枚举对象、字典、`DataTable`、`IDataReader` 和异步枚举导出，并报告进度。Rust 接受动态行或同类型 Serde 切片，并返回每张工作表的行数。 |
| 多工作表导出 | 部分实现 | Rust 可按输入顺序创建 visible、hidden 和 very-hidden 工作表，但尚不能在一次调用中接受异构 Serde 行类型。 |
| 修改现有工作簿 | 未实现 | 插入或替换工作表、复制并新增工作表，以及重命名、重排或修改工作表可见性。Rust 始终创建新的 XLSX 包。 |
| 模板 | 部分实现 | Rust 可使用标量和单 row 数组填充 path/byte 模板并保留 package part。stream、分组、条件、参数化 sheet、`$=` 公式、公式引用调整和 calculation chain 处理仍不支持。 |
| 图片与合并处理 | 未实现 | 添加锚定图片，以及通过模板 API 合并相邻相同单元格。结构化读取不等于具备写入能力。 |
| CSV | 未实现 | CSV 动态/强类型查询与保存、追加、列发现、DataReader/DataTable、分隔符/换行/编码/引号配置，以及 CSV/XLSX 转换。 |
| 批注与注释 | 未实现 | 读取线程化批注、回复、人员/作者、解决状态、时间戳和旧式注释。 |
| Fluent Mapping | 未实现 | 基于地址的对象映射、公式/格式映射、集合起始单元格与间距、嵌套集合，以及映射式导入/导出/模板 API。 |
| 特性式字段映射 | 部分实现 | 仍缺列索引/名称特性、本地化表头、公式元数据、自定义动态格式器、字段映射，以及动态列排序/过滤。Serde 可覆盖重命名、别名、默认值、跳过、可选值和自定义序列化；`WriteOptions` 可按最终 header name 配置 width/hidden layout。 |
| 读取配置 | 部分实现 | 区域文化感知转换、缓冲/快速模式，以及部分 null/空字符串行为。合并单元格填充和 shared-string 磁盘 cache 已实现。 |
| 写入配置与样式 | 部分实现 | 仍缺 OOXML table、共享字符串与内联字符串选择，以及更广泛的单元格样式。Rust 已暴露默认/最小 cell style 模式、header output/style、AutoFilter、从右到左 view、冻结行列、有界 AutoWidth、body 换行/对齐和数字格式。 |
| 工作表元数据与流程 | 部分实现 | 表格元数据、批注元数据、动态工作表别名、类级工作表选择，以及通过一个 reader 遍历所有工作表。Rust 已覆盖名称、顺序、尺寸、可见性和活动状态。 |
| Provider/包模型 | 设计不同 | .NET 组合 OpenXML、CSV、模板和 Fluent Mapping provider。Rust 使用单一 XLSX crate 加 CLI/WASM 适配器；这些适配器不能替代上述缺失能力。 |

## 证据索引

| 领域 | Rust 证据 | `../MiniExcel` 中的 .NET 证据 |
| --- | --- | --- |
| 公开读写边界 | `miniexcel/src/facade.rs`、`miniexcel/src/options.rs` | `src/MiniExcel.OpenXml/Api/OpenXmlImporter.cs`、`OpenXmlExporter.cs` |
| 表格 | 无公开表格 API | `OpenXmlImporter.QueryTableAsync`；`tests/MiniExcel.OpenXml.Tests/Tables/` |
| DataReader/DataTable | 无公开表格适配器 | `OpenXmlImporter.GetDataReader`、`GetAsyncDataReader`、`QueryAsDataTableAsync`；`tests/MiniExcel.OpenXml.Tests/DataReader/` |
| 多工作表与工作簿修改 | Writer 可在新工作簿中创建多个工作表；仍不支持编辑现有工作簿 | `OpenXmlExporter.InsertSheetAsync`、`CopyAndAddSheetAsync`、`AlterSheetAsync`；`MultipleSheets/` 与 `AlterSheets/` 测试 |
| 模板/图片/合并 | 已实现基础模板填充；高级指令与 authoring 仍延期 | `src/MiniExcel.OpenXml/Api/OpenXmlTemplater.cs`；`tests/MiniExcel.OpenXml.Tests/Templates/` |
| CSV/转换 | 核心仅支持 XLSX | `src/MiniExcel.Csv/Api/`；`src/MiniExcel/MiniExcelConverter.cs`；`tests/MiniExcel.Csv.Tests/` |
| 映射 | 仅 Serde 映射 | `src/MiniExcel.Core/Attributes/MiniExcelColumnAttribute.cs`；`src/MiniExcel.OpenXml.FluentMapping/`；映射测试 |
| 批注 | 无公开批注模型/API | `OpenXmlImporter.RetrieveCommentsAsync`；`src/MiniExcel.OpenXml/Models/Comments.cs`；批注测试 |
| 配置/样式 | 较窄的 `ReadOptions` 与 `WriteOptions` | `MiniExcelBaseConfiguration`、`OpenXmlConfiguration`、`OpenXmlStyleOptions`；导出测试 |

.NET 中标为 `Async` 的 API 还会通过仓库的同步版本生成机制产生公开同步入口，因此这里比较的是能力，而不只是方法命名差异。

## 建议实现顺序

1. **更丰富的写入选项**：增加隐藏工作表、表格/自动筛选、冻结窗格和样式控制，不必改动有界内存读取架构。
2. **命名表格查询与批注读取**：范围聚焦的 OpenXML 读取功能，容易建立明确 fixture 和公开结果模型。
3. **调用方提供的 `Read`/`Write` API**：先明确所有权与可 seek 约定，再设计异步包装。
4. **现有工作簿的工作表操作**：需要审慎设计包重写，并加强损坏防护与原子性测试。
5. **CSV provider**：应形成独立格式边界，不应在 XLSX parser 内堆叠条件分支。
6. **高级模板与 Fluent Mapping**：通过独立兼容里程碑增加分组/条件模板、参数化 sheet 和 mapping。

DataReader/DataTable 属于 .NET 生态抽象，不建议逐字移植。只有出现明确集成需求时，才应设计 Rust 原生的 record-batch 或表格适配器。

## 不计为缺口

- Rust 的分析与 RAG 导出属于 Rust 扩展，不是 .NET 等价声明。
- .NET 数字参数形式的 `QueryRange` 不单列，因为 Rust 的 A1 `CellReference` 边界可表达相同选择。
- 内部实现类和 .NET 专属依赖注入机制不计入，除非它们产生公开可观察行为。
- 旧式二进制 Excel 格式不计入，因为本次比对未发现当前 .NET V2 有可构成兼容要求的公开 provider。

任一基线 revision 变化后都应重新执行比对。只有两端公开适配器和聚焦测试均覆盖某项行为后，才应把新的等价声明加入共享兼容契约。