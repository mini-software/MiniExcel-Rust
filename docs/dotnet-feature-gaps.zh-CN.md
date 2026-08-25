# MiniExcel .NET 功能缺口分析

[English](dotnet-feature-gaps.md)

## 比对范围

本文比对以下本地检出的公开可观察能力。它是一份特定时间点的实现待办清单，并不表示 Rust 应逐项照搬所有 .NET API。

| 项目 | 版本基线 |
| --- | --- |
| MiniExcel-Rust | `23bba2d448bc4fab14baf750152a7c83cb6ececc`（`0.3.0`） |
| MiniExcel .NET | `b9a76d7af62142e0e38545b6905b01a06e8d160e` |

比对依据包括同级目录 `../MiniExcel` 中的 .NET 公开 API、控制实现和聚焦测试，以及 Rust 的公开 `MiniExcel` 门面、选项、集成测试和[兼容性边界](compatibility.zh-CN.md)。

状态定义：

- **已实现**：有实际价值的可观察 XLSX 行为已由 Rust-native API 与跨项目 focused evidence 覆盖。
- **部分实现**：Rust 已覆盖核心场景，但尚未覆盖 .NET 的完整可观察行为。
- **未实现**：Rust 当前没有对应的公开能力。
- **设计不同**：Rust 提供了原生替代方案，但不构成 API 或行为等价。

## 已实现基线

Rust 已支持动态及 Serde 强类型 XLSX 路径查询、闭区间 A1 范围、工作表选择、表头与空行选项、工作表名称/信息/尺寸、有界内存行迭代、结构化单元格来源信息、面向浏览器的字节读取，以及新建单工作表或多工作表 XLSX。除非 .NET 的对应能力明显更广，下文不把这些能力列为缺失。

## 缺口矩阵

| 领域 | 状态 | .NET 已有而 Rust 尚未完整实现的能力 |
| --- | --- | --- |
| 命名表格 | 已实现 | 动态/类型化 path query、byte query 与 borrowed-reader visitor 使用 table metadata header 和 bounds，并按 table name 大小写不敏感匹配。 |
| DataReader 与 DataTable | 设计不同 | Rust 使用 iterator 与 borrowed visitor，不复制 .NET tabular interface。只有具体集成需要时才考虑 Rust-native Arrow/record-batch adapter；此项不阻塞 parity 完成。 |
| 调用方提供的流 | 部分实现 | 已实现借用的同步动态/类型化/structured visitor、metadata 读取、动态/schema/类型化/多表 writer，以及独立 reader-to-writer Insert，并保持 leave-open。借用 lazy iterator、async stream 和 template stream 仍不支持。 |
| 异步与取消 | 部分实现 | 显式 schema path Insert 已支持可选、runtime-neutral 的 async row producer 与 cancellation。Async query/export/template、类型化 async source 和 progress callback 仍不支持；ZIP 与 filesystem 工作仍在专用 blocking worker 上执行。 |
| 通用保存输入 | 部分实现 | 从普通对象/可枚举对象、字典、`DataTable`、`IDataReader` 和异步枚举导出，并报告进度。Rust 接受动态行或同类型 Serde 切片，并返回每张工作表的行数。 |
| 多工作表导出 | 部分实现 | Rust 可按输入顺序创建 visible、hidden 和 very-hidden 工作表，但尚不能在一次调用中接受异构 Serde 行类型。 |
| 修改现有工作簿 | 部分实现 | Rust 可通过原子 path API 或独立 borrowed stream append 或严格 replace worksheet，并保留无关 package part 与 worksheet identity。复制新增、重命名、重排和独立 visibility 修改仍不支持。 |
| 模板 | 部分实现 | Rust 可使用标量和单 row 数组填充 path/byte 模板并保留 package part。stream、分组、条件、参数化 sheet、`$=` 公式、公式引用调整和 calculation chain 处理仍不支持。 |
| 图片与合并处理 | 未实现 | 添加锚定图片，以及通过模板 API 合并相邻相同单元格。结构化读取不等于具备写入能力。 |
| CSV | 未实现 | CSV 动态/强类型查询与保存、追加、列发现、DataReader/DataTable、分隔符/换行/编码/引号配置，以及 CSV/XLSX 转换。 |
| 批注与注释 | 已实现 | Path/bytes/borrowed API 返回 threaded root、reply、未解析 person ID、person/provider/user ID、resolved state、typed timestamp 与 legacy note。 |
| Fluent Mapping | 未实现 | 基于地址的对象映射、公式/格式映射、集合起始单元格与间距、嵌套集合，以及映射式导入/导出/模板 API。 |
| 特性式字段映射 | 部分实现 | 仍缺列索引/名称特性、本地化表头、公式元数据、自定义动态格式器、字段映射，以及动态列排序/过滤。Serde 可覆盖重命名、别名、默认值、跳过、可选值和自定义序列化；`WriteOptions` 可按最终 header name 配置 width/hidden layout。 |
| 读取配置 | 部分实现 | 区域文化感知转换、缓冲/快速模式，以及部分 null/空字符串行为。合并单元格填充和 shared-string 磁盘 cache 已实现。 |
| 写入配置与样式 | 部分实现 | 仍缺 OOXML table、共享字符串与内联字符串选择，以及更广泛的单元格样式。Rust 已暴露默认/最小 cell style 模式、header output/style、AutoFilter、从右到左 view、冻结行列、有界 AutoWidth、body 换行/对齐和数字格式。 |
| 工作表元数据与流程 | 部分实现 | 仍缺 dynamic sheet alias、class-level sheet selection 和单 reader 遍历全部 sheet。Rust 已覆盖名称、顺序、尺寸、可见性、active state、table metadata 与 comments/notes。 |
| Provider/包模型 | 设计不同 | .NET 组合 OpenXML、CSV、模板和 Fluent Mapping provider。Rust 使用单一 XLSX crate 加 CLI/WASM 适配器；这些适配器不能替代上述缺失能力。 |

## 证据索引

| 领域 | Rust 证据 | `../MiniExcel` 中的 .NET 证据 |
| --- | --- | --- |
| 公开读写边界 | `miniexcel/src/facade.rs`、`miniexcel/src/options.rs` | `src/MiniExcel.OpenXml/Api/OpenXmlImporter.cs`、`OpenXmlExporter.cs` |
| 表格 | `MiniExcel::query_table*`；Rust focused test 使用完全相同的 `TestQueryTable.xlsx` fixture（SHA-256 `04F719BF9F9E99D9B437A8FB32F8111FD92580A1D29ACAD10B6ED128C0564501`） | `OpenXmlImporter.QueryTableAsync`；`tests/MiniExcel.OpenXml.Tests/Tables/` |
| Comments | `MiniExcel::get_comments*`；Rust focused test 使用 `TestCommentsAndNotes.xlsx`（SHA-256 `3A855CE896ED62DC27C91797432DD89EE081F07CD03AB05BF1B0CD745543A3FC`） | `OpenXmlImporter.RetrieveCommentsAsync`；`tests/MiniExcel.OpenXml.Tests/Comments/` |
| DataReader/DataTable | Rust iterator 与 borrowed visitor 是原生抽象；不计划逐字复制 .NET tabular adapter | `OpenXmlImporter.GetDataReader`、`GetAsyncDataReader`、`QueryAsDataTableAsync`；`tests/MiniExcel.OpenXml.Tests/DataReader/` |
| 多工作表与工作簿修改 | Writer 可创建多个工作表；现有 workbook 支持 append 与严格 replacement，并保留 package，显式 schema producer 采用有界资源处理 | `OpenXmlExporter.InsertSheetAsync`、`CopyAndAddSheetAsync`、`AlterSheetAsync`；`MultipleSheets/` 与 `AlterSheets/` 测试 |
| 模板/图片/合并 | 已实现基础模板填充；高级指令与 authoring 仍延期 | `src/MiniExcel.OpenXml/Api/OpenXmlTemplater.cs`；`tests/MiniExcel.OpenXml.Tests/Templates/` |
| CSV/转换 | 核心仅支持 XLSX | `src/MiniExcel.Csv/Api/`；`src/MiniExcel/MiniExcelConverter.cs`；`tests/MiniExcel.Csv.Tests/` |
| 映射 | 仅 Serde 映射 | `src/MiniExcel.Core/Attributes/MiniExcelColumnAttribute.cs`；`src/MiniExcel.OpenXml.FluentMapping/`；映射测试 |
| 批注 | `MiniExcel::get_comments*`；Rust focused test 使用 `TestCommentsAndNotes.xlsx`（SHA-256 `3A855CE896ED62DC27C91797432DD89EE081F07CD03AB05BF1B0CD745543A3FC`） | `OpenXmlImporter.RetrieveCommentsAsync`；`src/MiniExcel.OpenXml/Models/Comments.cs`；comment test |
| 配置/样式 | 较窄的 `ReadOptions` 与 `WriteOptions` | `MiniExcelBaseConfiguration`、`OpenXmlConfiguration`、`OpenXmlStyleOptions`；导出测试 |

.NET 中标为 `Async` 的 API 还会通过仓库的同步版本生成机制产生公开同步入口，因此这里比较的是能力，而不只是方法命名差异。

## 建议实现顺序

1. **CSV provider**：保持独立格式边界，不在 XLSX parser 内堆叠条件分支。
2. **Async query/export/template API**：延续 runtime-neutral producer/cancellation 模式，不把 blocking ZIP 工作描述成 async I/O。
3. **高级模板与 Fluent Mapping**：通过独立兼容里程碑增加分组/条件模板、参数化 sheet 和 mapping。
4. **剩余 workbook edit**：copy/add、rename、reorder 和独立 visibility mutation 需要各自的 preservation contract。

DataReader/DataTable 属于 .NET 生态抽象，明确不作为逐字 Rust parity 要求。只有出现具体集成需求时，才应设计 Rust-native record-batch 或 table adapter。

## 不计为缺口

- Rust 的分析与 RAG 导出属于 Rust 扩展，不是 .NET 等价声明。
- .NET 数字参数形式的 `QueryRange` 不单列，因为 Rust 的 A1 `CellReference` 边界可表达相同选择。
- 内部实现类和 .NET 专属依赖注入机制不计入，除非它们产生公开可观察行为。
- 旧式二进制 Excel 格式不计入，因为本次比对未发现当前 .NET V2 有可构成兼容要求的公开 provider。

任一基线 revision 变化后都应重新执行比对。只有两端公开适配器和聚焦测试均覆盖某项行为后，才应把新的等价声明加入共享兼容契约。