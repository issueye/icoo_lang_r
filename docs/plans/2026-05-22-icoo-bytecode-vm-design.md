# Icoo 字节码 VM 设计草案

日期：2026-05-22

## 1. 目标和边界

本文档为未来 Icoo 字节码 VM 做设计准备。当前主执行路径仍然是 AST backend；VM 设计短期不改变语言行为、不替换解释器、不修改生产代码。

VM 的第一目标不是追求最高性能，而是给后续维护提供更清晰的执行边界：

- 统一函数、方法、模块、类初始化和 native 调用的调用协议。
- 把表达式执行从递归 AST walk 收敛到可保存、可恢复的 frame。
- 为 async suspension/resume 提供不重复执行副作用的恢复点。
- 保留当前 `Value` 运行时协议，避免一次性重写对象模型、标准库和类型检查器。

非目标：

- 不在第一版 VM 中改变 Icoo 源语言语法。
- 不立即把 `Value` 从 `Rc<RefCell<...>>` 改成线程安全结构。
- 不在 VM MVP 中实现抢占式调度。
- 不把 Tokio 或其他 Rust 后端类型暴露给 Icoo 用户代码。

## 2. 总体架构

建议采用“双 backend、共享前端”的迁移路线：

```text
source
  -> lexer
  -> parser AST
  -> resolver
  -> typechecker
  -> backend
       +-- AST interpreter（当前默认）
       +-- Bytecode compiler -> VM（未来）
```

VM backend 初期仍从已检查的 AST 编译字节码，不新增独立 IR。等指令集稳定后，可以再引入更接近控制流图的中间表示。

运行期保持两层对象：

- `Chunk`：函数或模块顶层的字节码、常量池、局部槽元数据和 span 表。
- `Frame`：一次调用或模块执行的运行状态，包含 `ip + stack + locals + env + wait_source`。

## 3. 指令集草案

指令格式建议先使用紧凑但易调试的 enum，稳定后再考虑编码为字节数组。

### 3.1 常量和栈操作

```text
LoadConst index          # constants[index] 入栈
LoadNil
LoadBool value
Pop
Dup
Swap
```

说明：

- `LoadConst` 覆盖字符串、整数、浮点数和预编译函数对象。
- `Pop` 用于表达式语句和清理临时值。

### 3.2 局部变量、闭包和模块环境

```text
DefineLocal slot
LoadLocal slot
StoreLocal slot
LoadUpvalue slot
StoreUpvalue slot
CloseUpvalues from_slot
LoadGlobal name_index
StoreGlobal name_index
LoadModuleExport name_index
```

说明：

- 局部变量使用 slot 编号，`let/const/final` 的可变性和初始化状态由 slot metadata 或 `Environment` 侧表维护。
- 初期可继续复用当前 `Environment` 处理全局、模块、闭包和 final 规则；VM 局部槽只覆盖函数内部的热路径。
- `CloseUpvalues` 用于函数返回或 block 退出时把被闭包捕获的局部槽提升为堆值。

### 3.3 集合和模板

```text
BuildArray count
BuildMap count            # 栈上为 key/value 交错项，key 保持 String
Template count            # 弹出 count 个片段并拼接
```

说明：

- Map MVP 继续保持字符串 key。
- 模板字符串可以编译为常量文本和表达式结果交错入栈，最后执行 `Template`。

### 3.4 算术、比较和逻辑

```text
Negate
Not
Add
Subtract
Multiply
Divide
Remainder
Equal
NotEqual
Less
LessEqual
Greater
GreaterEqual
JumpIfFalse target
JumpIfTrue target
Jump target
```

说明：

- `and/or` 使用短路跳转编译，不需要专门指令。
- 算术和比较的类型错误继续复用当前运行时错误格式和 span。

### 3.5 属性、方法和调用

```text
GetProperty name_index
SetProperty name_index
GetSuper name_index
Call argc
CallMethod name_index argc
CallNative native_id argc
Return
```

说明：

- `GetProperty` 覆盖实例字段、实例方法、模块导出和内置类型方法。
- `CallMethod` 是后续优化项；MVP 可以先编译为 `GetProperty + Call`。
- `CallNative` 可在编译期已知 native module method 时使用，否则保留动态调用。

### 3.6 类和函数声明

```text
MakeFunction function_index
MakeClass name_index field_count method_count
Inherit
DefineField kind name_index type_index has_initializer
DefineMethod name_index
BindMethod name_index
```

说明：

- `MakeFunction` 创建 `Value::Function` 或未来的 `Value::BytecodeFunction`。
- 类字段声明需要保留 `FieldKind`、类型标注和可选初始化表达式。
- 方法仍然是函数值，绑定 `self` 的行为由 `GetProperty` 或 `BindMethod` 完成。

### 3.7 模块和导入

```text
ImportModule source_index alias_index
ImportNames source_index count
Export name_index
FinishModule
```

说明：

- 模块加载、缓存、循环依赖检测仍由 loader 负责。
- `Export` 记录当前环境中指定绑定的导出值。
- 第一版 VM 可以只编译单模块执行，文件模块图迁移作为第二阶段。

### 3.8 async、等待和调度

```text
MakeCoroutine function_index
Await
Yield
Sleep
Suspend wait_kind
Resume
```

说明：

- `Await` 弹出 `Task`，若已完成则推入结果；若未完成则保存当前 frame，设置等待源并把当前 task 挂到 awaiters。
- `Yield` 用于兼容当前底层语义，推荐用户代码继续使用 `await sleep(0)`。
- `Suspend` 是 VM 内部状态，不一定暴露为用户可见指令。

## 4. Frame Layout

建议 frame 使用如下结构：

```rust
struct VmFrame {
    function: BytecodeFunctionRef,
    ip: usize,
    stack_base: usize,
    locals: Vec<Slot>,
    env: EnvRef,
    wait_source: Option<WaitSource>,
    return_to: Option<FrameId>,
}
```

字段说明：

- `ip`：下一条待执行指令位置。任何可暂停点必须在副作用完成后更新到可恢复位置。
- `stack`：VM 可以使用全局 operand stack，并通过 `stack_base` 划分 frame；也可以先为每个 frame 保存独立 `Vec<Value>`，降低实现复杂度。
- `locals`：局部槽，包含值、初始化状态、绑定种类和可选类型信息。
- `env`：当前闭包或模块环境。MVP 可继续依赖现有 `Environment`，避免立刻重写闭包、模块和 `final` 规则。
- `wait_source`：当前 frame 暂停原因，例如等待 task、timer、native future 或外部 I/O。

推荐第一版 frame layout：

```text
Frame
  ip
  stack: Vec<Value>
  locals: Vec<Slot>
  env: EnvRef
  wait_source: Option<WaitSource>
```

这个布局牺牲少量性能，但便于序列化调试、错误定位和 async 恢复。

## 5. 调用边界

### 5.1 Function Call

函数调用创建新 frame：

1. 校验参数数量和运行时类型。
2. 创建 child environment 或局部槽数组。
3. 写入参数槽。
4. 把调用方 frame 保存在 VM call stack。
5. 执行 callee chunk，直到 `Return`、runtime error 或 suspension。

普通 `fn` 立即执行；`async fn` 调用不执行函数体，只创建 coroutine 值。

### 5.2 Class Call

类调用边界沿用当前语义：

1. 分配 instance，并按继承链创建声明字段。
2. 执行字段初始化表达式。
3. 查找并调用 `init`。
4. `init` 返回值对用户不可见，类调用结果始终是 instance。

VM 中类调用可以先走 `call_value` 兼容路径，后续再拆成专门 opcode。

### 5.3 Method Call

方法调用等价于：

```text
receiver.method(args...)
  -> GetProperty(receiver, "method")
  -> bind self
  -> Call(args)
```

`super.method(...)` 需要保存当前方法闭包中的 `self`，并从父类方法表开始查找。

### 5.4 Module Call Boundary

模块不是函数调用，但模块顶层执行可以视为一个特殊 frame：

- 模块 frame 有独立 `env`。
- 导入在执行前或执行中解析为只读绑定。
- 顶层 `return/yield/await` 继续由 resolver 拒绝。
- 执行结束后收集 `export` 声明对应的绑定值。

### 5.5 Native Call

native call 是 VM 与宿主能力的边界：

- 输入参数必须已经是 `Value`。
- native 返回 `IcooResult<Value>`。
- 权限模型、文件系统、网络、环境变量和 WebIno download 都应在 native 或 interpreter helper 边界检查。
- 同步 native 调用直接返回。
- 未来异步 native 调用返回 `WaitSource::NativeFuture`，VM 暂停当前 frame，完成后把结果推回 operand stack。

## 6. Async Suspension/Resume 模型

当前 AST backend 的协程已有 `instructions + pc` 雏形，但复杂表达式中的 `await` 可能难以保证恢复时不重复执行副作用。VM 的核心收益是把恢复点放在指令级。

### 6.1 Task 状态

语言层 task 状态保持：

```text
Queued -> Running -> Waiting -> Queued -> Done
                         +-----> Failed
                         +-----> Cancelled
```

VM frame 额外保存：

```text
WaitSource::Task(task_id)
WaitSource::Timer(timer_id)
WaitSource::Native(native_id)
WaitSource::Yield
```

### 6.2 Await 流程

`Await` 指令流程：

1. 从栈顶弹出待等待值。
2. 如果不是 `Task`，抛运行时错误。
3. 如果目标 task 已完成，把结果推回栈，继续执行下一条指令。
4. 如果目标 task 失败或取消，把错误传播到当前 frame。
5. 如果目标 task 未完成：
   - 当前 frame 的 `ip` 已经指向 `Await` 后一条指令。
   - 当前 task 状态设为 `Waiting`。
   - `wait_source = Task(target_id)`。
   - 当前 task 加入目标 task awaiters。
   - VM 返回调度器。

目标 task 完成时，调度器唤醒 awaiters；恢复当前 frame 前把目标结果推回栈。这样 `let value = await task` 的后续 `StoreLocal` 只执行一次。

### 6.3 Sleep 和 Yield

`sleep(ms)` 可以继续实现为返回一个 timer task：

- `await sleep(0)` 表示主动让出，让当前 task 重新入队。
- `await sleep(ms)` 注册 timer，到期后唤醒 task。

`yield` 保留为底层能力，可编译为 `Suspend(Yield)`，不建议扩展为表达式级语义，除非后续引入生成器协议。

### 6.4 Resume Invariants

VM 恢复必须满足：

- 可暂停指令在暂停前完成所有必要状态保存。
- `ip` 指向恢复后应执行的下一条指令。
- operand stack 中保留暂停前已计算、恢复后仍需要的值。
- native 调用如果已经产生宿主副作用，恢复时不得再次调用。
- runtime error span 使用暂停点或原始 AST 节点 span 映射。

## 7. AST Backend 到 VM 的迁移策略

### 7.1 阶段 0：等价测试骨架

新增 `tests/backend_equivalence.rs`，先只跑 AST backend。所有样例按同步语言子集组织，未来 VM backend 接入后同一批样例跑双 backend。

第一批覆盖：

- 字面量、算术、比较、逻辑。
- `let/const/final` 和赋值。
- `if/elif/else`、`while`、`break/continue`。
- 函数、递归、闭包。
- Array/Map 基础方法。
- class、inheritance、self/super、声明字段。
- 模块导入导出可以第二批加入，因为涉及测试文件布局和 loader 差异。

### 7.2 阶段 1：只编译同步表达式和语句

先支持无函数调用栈之外的同步子集：

- literals
- unary/binary/logical
- local bindings
- if/while
- arrays/maps/templates
- print 输出

这个阶段可以提供内部 `run_source_with_backend(source, Backend::Vm)`，但不改变公开默认入口。

### 7.3 阶段 2：函数、闭包和类

迁移函数和类时保持 `Value` 协议：

- 新增 `Value::BytecodeFunction` 或让 `IcooFunction` 持有可选 bytecode body。
- 方法绑定继续产出可调用值。
- 类字段和继承复用当前 `IcooClass`/`Instance`。

AST backend 和 VM backend 必须在错误信息、输出顺序、final 赋值规则上保持一致。

### 7.4 阶段 3：模块和 native 标准库

模块迁移应在函数和类稳定后进行：

- loader 仍共享。
- 每个模块单独编译 chunk。
- native module method 保持同一注册表和权限检查。
- `Value::Module` 导出表仍是共享协议。

### 7.5 阶段 4：async VM

最后迁移 async：

- 把当前 `CoroutineInstr` 改为 bytecode chunk + frame。
- `Task` 持有 suspended frame。
- `EventLoop` 调度 frame，而不是 AST statement PC。
- 放宽 `await` 位置前，必须通过副作用不重复执行的等价测试。

## 8. 错误和调试信息

每个 chunk 需要维护 `ip -> Span` 表。运行时错误使用当前 `ip` 对应 span；编译器应在生成每条可能报错的指令时记录源 AST span。

建议调试输出：

```text
== function add ==
0000 LoadLocal 0      @ line 2 col 12
0001 LoadLocal 1
0002 Add
0003 Return
```

这能让 VM 早期测试失败时快速定位 AST 编译问题。

## 9. 待确认问题

- 是否新增 `Value::BytecodeFunction`，还是扩展 `IcooFunction` body 表示。
- VM 局部槽是否第一版就完全替代 `Environment`，还是先混合执行。
- 模块顶层是否需要独立 `ModuleChunk` 类型。
- `await` 是否在 VM async 稳定后重新允许任意表达式位置。
- native async 返回值使用 `Task` 统一表达，还是引入内部-only future wait source。
