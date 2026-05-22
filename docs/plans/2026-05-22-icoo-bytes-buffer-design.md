# Icoo Bytes/Buffer Design

**Goal:** 为 Icoo 增加清晰的二进制数据模型，覆盖文件 I/O、HTTP client、WebIno 上传下载和流式响应，同时保持现有 `String` 文本 API 的兼容性。

**Scope:** 本文只做设计，不修改 Rust 代码。后续实现应拆成独立小步，避免和权限、HTTP、WebIno worker 同时改同一段解释器代码。

---

## 1. 结论：新增 Bytes 与 Buffer

建议同时新增两个内置类型：

```text
Bytes   # 不可变二进制序列
Buffer  # 可变二进制缓冲区
```

`Bytes` 是默认二进制值类型，适合函数返回值、Map/Array 中传递、跨模块边界、HTTP body、文件内容和上传文件内容。语义上接近不可变 `String`，一旦创建后内容不可修改。任何派生操作，例如 `slice()`、`concat()`、`from_hex()`，都返回新的 `Bytes`。

`Buffer` 是显式的可变构建器，适合逐块读取、拼接上传体、构造协议报文和减少中间拷贝。它不应作为默认 API 返回类型，除非 API 明确表示调用者会继续追加或原地修改。`Buffer.to_bytes()` 冻结快照，返回不可变 `Bytes`；`Buffer.clear()`、`Buffer.append()` 修改自身。

`String` 继续表示 UTF-8 文本，不再承担任意二进制容器职责。`String` 与 `Bytes` 之间只通过显式编码转换：

```text
text.to_bytes() -> Bytes                  # UTF-8 编码
bytes.to_string() -> String               # 仅 UTF-8 成功时返回文本
bytes.to_string("lossy") -> String         # 非 UTF-8 用替换字符
bytes.to_string("hex") -> String           # 调试显示，等价 to_hex()
```

推荐规则：

- 文本 API 继续使用 `String`，例如 `read_text()`、`write_text()`、HTTP 默认 `body`。
- 二进制 API 使用 `Bytes`，例如 `read_bytes()`、`write_bytes()`、上传文件内容、下载响应体。
- 需要大量增量拼接时由用户显式创建 `Buffer`，最后转成 `Bytes`。
- 不允许把 `Bytes` 隐式当作 `String` 拼接或模板字符串插值；插值时调用 `to_string()` 的调试格式，避免误把二进制写成文本。

---

## 2. 语法和类型标注示例

第一阶段不需要新增字面量语法，可以用构造函数和编码方法创建二进制值：

```text
import "std.io.fs" as fs

let data: Bytes = fs.read_bytes("logo.png")
let copy: Bytes = data.slice(0, data.len())
let hex: String = data.to_hex()
let restored: Bytes = Bytes.from_hex(hex)

fs.write_bytes("logo.copy.png", restored)
```

字符串转 UTF-8 bytes：

```text
let text: String = "hello"
let payload: Bytes = text.to_bytes()
print(payload.len())
print(payload.to_string())
```

显式处理 UTF-8 失败：

```text
let raw: Bytes = Bytes.from_hex("fffe00")

try:
    print(raw.to_string())
catch err:
    print("not utf-8")

print(raw.to_string("lossy"))
print(raw.to_hex())
```

使用 `Buffer` 构造请求体：

```text
let buf: Buffer = Buffer.new()
buf.append("name=".to_bytes())
buf.append("icoo".to_bytes())
buf.append(Bytes.from_hex("0d0a"))

let body: Bytes = buf.to_bytes()
```

未来可以考虑 bytes 字面量，但不建议第一阶段实现：

```text
# Future only
let magic: Bytes = b"\x89PNG\r\n\x1a\n"
let raw: Bytes = x"89504e470d0a1a0a"
```

类型检查规则：

- `Bytes` 与 `Buffer` 是一等类型，可用于变量、参数、返回值、Array、Map value、Task 泛型。
- `Bytes` 不可赋给 `String`，`String` 不可赋给 `Bytes`，必须显式转换。
- `Buffer` 不可隐式赋给 `Bytes`，必须调用 `to_bytes()`。
- `equals()` 支持 `Bytes` 对 `Bytes`、`Buffer` 对 `Buffer`、`Buffer` 对 `Bytes`，但 `==` 是否支持二进制比较可后置；第一阶段可先实现方法。

---

## 3. 内置方法设计

### Bytes

```text
Bytes
  to_string() -> String
  to_string(mode: String) -> String
  type_name() -> String
  len() -> Int
  is_empty() -> Bool
  slice(start: Int, end: Int?) -> Bytes
  concat(other: Bytes) -> Bytes
  equals(other: Bytes) -> Bool
  to_hex() -> String
  to_base64() -> String
```

静态构造方法：

```text
Bytes.from_hex(value: String) -> Bytes
Bytes.from_base64(value: String) -> Bytes
Bytes.from_string(value: String) -> Bytes
Bytes.empty() -> Bytes
```

行为细节：

- `len()` 返回字节数，不是字符数。
- `slice(start, end)` 使用半开区间 `[start, end)`，单位为字节；负数索引第一阶段不支持，越界报运行时错误。
- `concat(other)` 返回新 `Bytes`，不会修改接收者。
- `equals(other)` 做字节级相等比较。
- `to_string()` 默认按 UTF-8 严格转换，失败时报运行时错误。
- `to_string("strict")` 等价默认行为。
- `to_string("lossy")` 使用 UTF-8 lossless/lossy 策略，非法序列替换为 `U+FFFD`。
- `to_string("hex")` 返回 `to_hex()`，用于日志和调试。
- `to_hex()` 输出小写十六进制，不带 `0x` 前缀。
- `from_hex()` 接受偶数长度十六进制字符串，可忽略 ASCII 空白；遇到非法字符报错。
- `to_base64()` 使用标准 Base64 带 padding。
- `from_base64()` 接受标准 Base64，非法输入报错。

`Bytes.to_string()` 的展示格式需要谨慎。通用 `print(bytes)` 或插值不应尝试严格 UTF-8，否则日志可能因为二进制内容中断。建议：

```text
bytes.to_string()        # 严格 UTF-8 文本转换
str(bytes) / display     # "<bytes len=12 hex=89504e47...>"
```

如果当前运行时的 `to_string()` 和 display 是同一入口，第一阶段可让 `Bytes.to_string()` 返回调试格式，并新增 `decode_utf8()`。但从语言一致性看，更推荐后续拆分：`display()` 负责安全展示，`to_string()` 负责显式文本转换。

### Buffer

```text
Buffer
  to_string() -> String
  type_name() -> String
  len() -> Int
  is_empty() -> Bool
  append(value: Bytes) -> Buffer
  append_string(value: String) -> Buffer
  slice(start: Int, end: Int?) -> Bytes
  to_bytes() -> Bytes
  clear() -> Nil
  equals(other: Bytes|Buffer) -> Bool
  to_hex() -> String
  to_base64() -> String
```

静态构造方法：

```text
Buffer.new() -> Buffer
Buffer.from_bytes(value: Bytes) -> Buffer
Buffer.from_string(value: String) -> Buffer
```

行为细节：

- `append(value)` 接受 `Bytes`；是否接受 `Buffer` 可以第二阶段再做。
- `append_string(value)` 明确使用 UTF-8 编码追加文本。
- `slice()` 返回不可变 `Bytes`，避免暴露内部可变存储。
- `to_bytes()` 返回当前内容快照；之后继续修改 `Buffer` 不影响已返回的 `Bytes`。
- `to_string()` 与 `Bytes.to_string()` 规则一致，但基于当前内容。

---

## 4. std.io.fs 二进制 API

新增方法：

```text
fs.read_bytes(path: String) -> Bytes
fs.write_bytes(path: String, content: Bytes) -> Nil
fs.append_bytes(path: String, content: Bytes) -> Nil
```

兼容现有文本 API：

```text
fs.read_text(path: String) -> String
fs.write_text(path: String, content: String) -> Nil
fs.append_text(path: String, content: String) -> Nil
```

行为设计：

- `read_bytes()` 读取原始文件字节，不做 UTF-8 转换。
- `write_bytes()` 覆盖写入原始字节；父目录不存在时沿用当前 `write_text()` 的错误行为，不自动创建目录。
- `append_bytes()` 不存在则创建文件，存在则追加原始字节。
- 权限模型与文本 API 对齐：`read_bytes()` 检查 `fs_read`，`write_bytes()` 和 `append_bytes()` 检查 `fs_write`。
- 错误消息使用独立方法名，例如 `io.fs.read_bytes() failed: ...`，方便测试和定位。
- 大文件第一阶段仍一次性读入内存，后续再设计 streaming file API。

示例：

```text
import "std.io.fs" as fs

let image: Bytes = fs.read_bytes("in.png")
fs.write_bytes("out.png", image)
fs.append_bytes("log.bin", Bytes.from_hex("0d0a"))
```

---

## 5. HTTP Client 与 WebIno 兼容策略

### std.net.http.client

现有接口以 `String` body 为主，应保持兼容：

```text
client.post(url: String, body: String, headers: Map<String, String>?) -> Map
client.stream_get(url: String, headers: Map<String, String>?, handler: Function) -> Map
```

新增二进制友好接口有两种选择：

方案 A：在现有 `post`/`put` 中接受 `String|Bytes` body。

```text
client.post(url, body: Bytes, headers)
```

方案 B：新增显式方法。

```text
client.post_bytes(url: String, body: Bytes, headers: Map<String, String>?) -> Map
client.put_bytes(url: String, body: Bytes, headers: Map<String, String>?) -> Map
client.get_bytes(url: String, headers: Map<String, String>?) -> Map
```

推荐先做方案 B，原因是当前类型检查器对联合类型支持有限，显式方法更容易测试，也不会改变现有 `post()` 的错误边界。

返回值建议逐步扩展：

```text
{
  status: Int,
  headers: Map<String, String>,
  body: String,       # 兼容字段，按文本响应保留
  body_bytes: Bytes   # 新字段，始终是原始响应体
}
```

二进制接口：

- `get_bytes()`、`post_bytes()`、`put_bytes()` 的 `body_bytes` 是主字段。
- 为兼容 Map 结构，可以仍提供 `body`，但只有响应体是合法 UTF-8 时填入文本；否则设为 `""` 或不填。推荐不填 `body` 会破坏旧式字段预期，第一阶段可填 `body = body_bytes.to_string("lossy")`，并在文档标注二进制调用应使用 `body_bytes`。
- `Content-Length` 必须按字节长度计算。
- `Content-Type` 对 `String` body 默认继续是 `text/plain; charset=utf-8`；对 `Bytes` body 默认 `application/octet-stream`，除非 headers 已提供 `Content-Type`。

流式响应：

现有 stream handler 接收 `String` chunk。为兼容保留：

```text
client.stream_get(url, handler: fn(chunk: String))
```

新增 bytes stream 方法：

```text
client.stream_get_bytes(url: String, headers: Map<String, String>?, handler: Function) -> Map
client.stream_post_bytes(url: String, body: Bytes, headers: Map<String, String>?, handler: Function) -> Map
```

handler 每次收到 `Bytes`：

```text
fn on_chunk(chunk: Bytes):
    print(chunk.len())
```

流式返回 Map 保持：

```text
{
  status: Int,
  headers: Map<String, String>,
  chunks: Int
}
```

### std.web.ino

请求 Map 扩展：

```text
req["body"] -> String
req["body_bytes"] -> Bytes
req["files"][field]["content"] -> String
req["files"][field]["content_bytes"] -> Bytes
```

兼容策略：

- 现有 `req["body"]` 保持 String，以免破坏路由测试和表单场景。
- 新增 `req["body_bytes"]` 保存原始请求体。
- multipart 上传文件新增 `content_bytes`，原 `content` 保留为 lossy UTF-8 文本，适合旧代码和简单表单。
- multipart 解析应基于原始 bytes 寻找 boundary，避免二进制上传经过 `String::from_utf8_lossy()` 后损坏。

响应方法扩展：

```text
res.send(value: String|Bytes|Any) -> Nil
res.write(value: String|Bytes) -> WebInoResponse
res.download(path: String, filename: String?) -> Nil
res.send_bytes(value: Bytes, content_type: String?) -> Nil
```

推荐落地顺序：

- 第一阶段新增 `send_bytes()` 和 `write_bytes()`，不改变 `send()`/`write()` 参数类型。
- 第二阶段如果类型系统支持联合类型，再让 `send()`/`write()` 接受 `Bytes`。

下载：

- `res.download()` 当前已经使用原始 bytes 发送文件，未来只需让内部表示统一到 `Bytes`。
- 下载响应的 `Content-Length` 使用字节长度。
- `Content-Disposition` 文件名继续做 header 注入防护。

流式响应：

```text
res.write("hello")
res.write_bytes(Bytes.from_hex("000102"))
res.end()
```

兼容策略：

- 文本 chunk 仍按 UTF-8 写入。
- bytes chunk 原样写入。
- 如果一次响应混用文本和 bytes，允许，但 `Content-Type` 由用户显式设置；未设置时 bytes stream 默认 `application/octet-stream`，纯文本 stream 默认 `text/plain; charset=utf-8`。

---

## 6. 安全与性能

### 拷贝策略

`Bytes` 的运行时表示建议使用共享不可变存储，例如 Rust 侧 `Rc<Vec<u8>>` 或后续可切换的 `bytes::Bytes`。第一阶段为降低依赖和改动面，可以先用 `Rc<Vec<u8>>`：

- `clone(Bytes)` 只增加引用计数，不复制底层字节。
- `slice()` 第一阶段可复制切片以简化实现；后续再优化为共享切片视图。
- `concat()` 分配新 Vec，容量为两段长度之和。
- `to_bytes()` 从 `Buffer` 返回快照时必须复制或转移所有权，保证之后修改 `Buffer` 不影响 `Bytes`。

`Buffer` 建议使用 `Rc<RefCell<Vec<u8>>>`，与当前 `Array`/`Map` 的可变模型一致。注意它不应跨线程共享；若未来 Task/thread 需要移动值，应禁止 `Buffer` 跨线程或先冻结成 `Bytes`。

### 最大体积

需要为一次性读入内存的 API 设软上限，避免脚本无意读取超大文件或响应：

```text
默认单个 Bytes 最大体积：64 MiB
默认单个 HTTP request/response body 最大体积：16 MiB
默认单个 WebIno request body 最大体积：16 MiB
默认单个 stream chunk 最大体积：64 KiB
```

这些上限应集中到运行时配置，后续由 CLI 或 embedding API 覆盖。超过限制时报运行时错误，例如：

```text
bytes value exceeds maximum size: 67108864 bytes
http response body exceeds maximum size: 16777216 bytes
```

第一阶段如果还没有运行时配置，可以先定义常量，并在文档和测试中固定预期。

### 权限

二进制 API 不引入新权限种类，沿用既有宿主能力权限：

- `fs.read_bytes()`：`fs_read`
- `fs.write_bytes()`、`fs.append_bytes()`：`fs_write`
- HTTP client bytes API：`net_connect`
- WebIno listen 和 bytes body：`net_listen`
- `res.download()` 继续检查 `fs_read`

编码转换 `to_hex()`、`from_hex()`、`to_base64()`、`from_base64()` 不需要宿主权限，但仍受最大体积限制。

### UTF-8 转换失败

必须区分“严格文本转换”和“安全展示”：

- `Bytes.to_string()` 默认严格 UTF-8，失败报错。
- `Bytes.to_string("lossy")` 永不因 UTF-8 失败报错。
- `print(bytes)`、模板插值和错误消息中的 bytes 展示应使用安全调试格式，不读取全部内容。
- `fs.read_text()` 继续严格 UTF-8 读取，失败报错；需要读取任意文件时使用 `read_bytes()`。
- HTTP 文本接口可继续使用 lossy 策略以兼容当前行为，但 bytes 接口必须保留原始字节。

### Header 与协议安全

- bytes body 不改变现有 header CRLF 注入检查。
- `Content-Length` 永远按字节数计算，不能用字符串字符数。
- Base64/hex 解码错误不能 panic，必须返回 Icoo runtime error。
- `from_hex()` 和 `from_base64()` 应在解码前后检查最大体积，避免超大字符串造成内存压力。

---

## 7. 分阶段落地计划

### Phase 0：只补设计和测试占位

**Files:**

- Create: `docs/plans/2026-05-22-icoo-bytes-buffer-design.md`

**Verification:**

```text
cargo test
```

### Phase 1：运行时类型与基础方法

**Files likely touched:**

- Modify: `src/runtime/value.rs`
- Modify: `src/interpreter/mod.rs`
- Modify: `src/typechecker.rs`
- Test: `tests/bytes_buffer.rs`

**Tasks:**

1. 新增 `Value::Bytes` 与 `Value::Buffer`。
2. 新增类型名、display、truthy/equality 基础行为。
3. 实现 `Bytes.empty/from_hex/from_base64/from_string`。
4. 实现 `Bytes.len/slice/concat/equals/to_hex/to_base64/to_string`。
5. 实现 `Buffer.new/from_bytes/from_string/append/append_string/slice/to_bytes/clear`。
6. 补类型检查器方法返回值和参数检查。

### Phase 2：std.io.fs 二进制 API

**Files likely touched:**

- Modify: `src/native_modules/io_fs.rs`
- Modify: `src/typechecker.rs`
- Test: `tests/native_modules_matrix.rs`
- Test: `tests/bytes_io_fs.rs`

**Tasks:**

1. 注册 `read_bytes/write_bytes/append_bytes`。
2. 接入 `fs_read/fs_write` 权限。
3. 增加二进制 round-trip 测试。
4. 增加 UTF-8 失败时 `read_text()` 报错、`read_bytes()` 成功的测试。

### Phase 3：HTTP client bytes API

**Files likely touched:**

- Modify: `src/native_modules/net_http_client.rs`
- Modify: `src/interpreter/http_client.rs`
- Modify: `src/typechecker.rs`
- Test: `tests/http_client_bytes.rs`

**Tasks:**

1. 新增 `get_bytes/post_bytes/put_bytes`。
2. 返回 Map 增加 `body_bytes`。
3. `Content-Length` 按 bytes 长度计算。
4. 新增 `stream_get_bytes/stream_post_bytes/stream_put_bytes`，handler 接收 `Bytes`。
5. 保持旧 stream API handler 接收 `String`。

### Phase 4：WebIno bytes body、上传和响应

**Files likely touched:**

- Modify: `src/interpreter/web_ino.rs`
- Modify: `src/runtime/value.rs`
- Modify: `src/typechecker.rs`
- Test: `tests/web_ino_bytes.rs`
- Test: `tests/web_ino_response_headers.rs`

**Tasks:**

1. 请求读取从 `String` 改为原始 bytes，再派生文本字段。
2. `req["body_bytes"]` 保存原始 body。
3. multipart 文件保存 `content_bytes`。
4. 新增 `res.send_bytes()` 和 `res.write_bytes()`。
5. 保持 `res.download()` 行为，并统一内部 bytes 表示。

### Phase 5：性能与限制配置

**Files likely touched:**

- Modify: `src/runtime/mod.rs`
- Modify: `src/runtime/value.rs`
- Modify: `src/interpreter/http_client.rs`
- Modify: `src/interpreter/web_ino.rs`
- Test: `tests/bytes_limits.rs`

**Tasks:**

1. 集中定义 bytes/http/web body 最大体积。
2. 在构造、解码、文件读、HTTP 读、WebIno body 读处检查限制。
3. 补超限错误测试。
4. 评估是否引入 `bytes::Bytes` 优化 `slice()`。

---

## 8. 测试清单

基础类型：

- `Bytes.empty().len() == 0`
- `Bytes.from_hex("00ff10").to_hex() == "00ff10"`
- `Bytes.from_hex("00 ff 10")` 支持空白
- `Bytes.from_hex("0")` 报错
- `Bytes.from_hex("xx")` 报错
- `Bytes.from_base64(data.to_base64()).equals(data)`
- `slice(0, len)` 返回相同内容
- `slice()` 越界报错
- `concat()` 不修改原对象
- `to_string()` 对合法 UTF-8 成功
- `to_string()` 对非法 UTF-8 报错
- `to_string("lossy")` 对非法 UTF-8 成功
- `print(bytes)` 不因非法 UTF-8 报错

Buffer：

- `Buffer.new().is_empty() == true`
- `append()` 后 len 增加
- `append_string()` 使用 UTF-8 字节长度
- `to_bytes()` 返回快照，之后 `clear()` 不影响快照
- `slice()` 返回 `Bytes`
- `equals()` 可比较 `Bytes`

文件：

- `write_bytes()` + `read_bytes()` round-trip 任意 bytes
- `append_bytes()` 对不存在文件创建
- `append_bytes()` 对已有文件追加
- `read_text()` 读取非法 UTF-8 文件报错
- `read_bytes()` 读取非法 UTF-8 文件成功
- 权限关闭 `fs_read` 时 `read_bytes()` 拒绝
- 权限关闭 `fs_write` 时 `write_bytes()`/`append_bytes()` 拒绝

HTTP client：

- `post_bytes()` 发送 `00 ff` 时服务端收到原始 bytes
- `Content-Length` 等于字节数
- 用户提供 `Content-Type` 时不覆盖
- 未提供 `Content-Type` 时 bytes body 默认 `application/octet-stream`
- `get_bytes()` 对二进制响应保留 `body_bytes`
- 旧 `get()`/`post()` 行为不变
- `stream_get_bytes()` handler 接收 `Bytes`
- 旧 `stream_get()` handler 仍接收 `String`
- `net_connect` 权限关闭时 bytes API 拒绝

WebIno：

- `req["body"]` 兼容旧 String 行为
- `req["body_bytes"]` 保存原始 bytes
- multipart 上传二进制文件时 `content_bytes` 不损坏
- multipart 旧 `content` 字段仍存在
- `res.send_bytes()` 发送原始 bytes
- `res.write_bytes()` 可流式发送原始 bytes
- `res.download()` 仍发送原始文件 bytes
- `Content-Length` 使用 bytes 长度
- `net_listen` 权限关闭时 WebIno bytes 场景同样拒绝

限制与错误：

- 超过最大 bytes 体积时报错
- 超过 HTTP body 限制时报错
- 超过 WebIno request body 限制返回 400 或运行时错误，行为需固定
- Base64/hex 非法输入返回 Icoo runtime error，不 panic

---

## 9. 主要设计取舍

- `Bytes` 和 `Buffer` 都需要，但默认 API 应优先返回不可变 `Bytes`，降低别名修改风险。
- `String` 只表示文本，二进制和文本之间必须显式转换。
- 第一阶段不新增 bytes 字面量，先用 `Bytes.from_hex()`、`Bytes.from_base64()` 和 `String.to_bytes()` 覆盖需求。
- 文件 API 新增显式 `read_bytes/write_bytes/append_bytes`，不改变现有文本 API。
- HTTP client 推荐新增 `*_bytes` 方法，而不是立刻让现有方法接受联合类型。
- WebIno 兼容旧 `body`/`content` 字段，同时新增 `body_bytes`/`content_bytes`。
- 严格 UTF-8 转换和安全调试展示必须分离，避免二进制日志导致运行时错误。
- 第一版可以一次性读入内存，但必须定义最大体积；真正流式文件 I/O 留到后续设计。
