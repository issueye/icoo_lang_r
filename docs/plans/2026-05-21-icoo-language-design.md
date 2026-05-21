# Icoo 脚本语言设计文档

日期：2026-05-21

## 1. 目标

Icoo 是一个使用 Rust 实现的小型脚本语言。语法风格接近 Python，但对象模型更严格：类属性必须显式声明，继承使用 `class Child <- Parent:` 语法。

第一版推荐实现为 AST 解释器。这样在语言语法和运行时语义还未完全稳定时，修改成本更低，也更容易调试。等核心行为稳定后，再考虑增加字节码虚拟机。

主要目标：

- 简单的 Python 风格语法。
- 使用缩进表示代码块。
- 支持变量和常量。
- 支持 `final` 一次赋值绑定。
- 支持函数和方法。
- 支持语言级命名规范：常量大写、类名首字母大写、方法名使用下划线风格。
- 支持类，并且类属性必须显式声明。
- 支持单继承，继承语法使用 `<-`。
- 支持可选类型标注。
- 所有数据类型都自带基础方法，例如 `to_string()`。
- 支持协程和事件循环设计，用于可暂停、可恢复的执行流程，并避免运行时被单线程调度瓶颈限制。
- 运行时报错信息带源码位置。

第一版不做：

- 多继承。
- 泛型。
- 完整异步 I/O 运行时。
- 装饰器。
- 完整静态类型系统。
- 复杂模块系统。

## 2. 语言示例

```python
const PI: Float = 3.14159

let version: String = "0.1.0"
let count = 10
final runtime_id: String
runtime_id = "icoo-" + count.to_string()

fn add(a: Int, b: Int) -> Int:
    return a + b

class Animal:
    let name: String

    fn init(self, name: String):
        self.name = name

    fn speak(self):
        print("...")

class Dog <- Animal:
    let breed: String
    final owner_id: String

    fn init(self, name: String, breed: String, owner_id: String):
        super.init(name)
        self.breed = breed
        self.owner_id = owner_id

    fn speak(self):
        print(self.name + " says woof")

let dog = Dog("Lucky", "Border Collie", "U001")
dog.speak()

print(count.to_string())
print(dog.to_string())
```

## 3. 类型设计

Icoo 采用“动态执行 + 可选类型标注”的设计。第一版即使不做完整静态类型检查，也应该在语法分析阶段保留类型标注，方便后续扩展。

内置类型：

```text
Nil
Bool
Int
Float
String
Array
Map
Function
Coroutine
Task
EventLoop
Class
Instance
Any
```

字符串：

```python
let single_line = "hello"
let multi_line = """
hello
world
"""
let name = "Icoo"
let message = f"hello {name}"
let report = f"""
name={name}
version={version}
"""
```

规则：

- 普通字符串使用双引号 `"..."`。
- 多行字符串使用三引号 `"""..."""`，内容可以跨行。
- 字符串模板使用 `f"..."` 或 `f"""..."""`。
- 模板中 `{表达式}` 会执行表达式并转换为字符串。
- 模板中 `{{` 表示字面量 `{`，`}}` 表示字面量 `}`。

变量声明：

```python
let age: Int = 18
let name = "Tom"
```

常量声明：

```python
const MAX_COUNT: Int = 100
```

一次赋值绑定：

```python
final config_path: String
config_path = "./config.icoo"
config_path = "./other.icoo"  # 错误：final 只能第一次赋值
```

规则：

- `let` 声明可变绑定。
- `const` 声明不可变绑定，并且必须在声明时初始化。
- `const` 必须初始化。
- 对 `const` 重新赋值是运行时错误。
- `final` 声明一次赋值绑定。
- `final` 可以声明时初始化，也可以延迟到后续第一次赋值。
- `final` 如果没有声明时初始化，则必须写类型标注。
- 读取尚未初始化的 `final` 是运行时错误。
- 对已经初始化的 `final` 再次赋值是运行时错误。
- 局部变量的类型标注可选。
- 类属性建议第一版强制写类型标注，降低实现复杂度，也让对象结构更清晰。

## 4. 命名规范

Icoo 在语言层面对部分命名做强约束。违反命名规范应在 Parser 或 Resolver 阶段报错，推荐放在 Resolver 阶段统一检查，这样错误信息可以结合声明类型给出更清晰的提示。

常量命名：

```python
const MAX_COUNT: Int = 100
const PI: Float = 3.14159
const _DEBUG_: Bool = true
```

规则：

- `const` 声明的名称只能使用大写字母、数字和下划线。
- `const` 声明的名称不能包含小写字母。
- `const` 声明的名称必须至少包含一个大写字母。
- `const` 声明的名称不能以数字开头。
- 第一版“特殊符号”只允许 `_`，不允许把 `+`、`-`、`*`、`$` 等运算符或符号作为标识符的一部分，避免和词法规则冲突。

非法示例：

```python
const max_count = 100     # 错误：包含小写字母
const version = "0.1.0"   # 错误：包含小写字母
const _ = 1               # 错误：没有大写字母
const 1_COUNT = 1         # 错误：不能以数字开头
```

类名命名：

```python
class User:
    let name: String

class HttpClient:
    let base_url: String
```

规则：

- 类名首字母必须大写。
- 推荐使用 PascalCase，例如 `UserProfile`、`HttpClient`。
- 第一版只强制首字母大写，不强制后续字符必须完全符合 PascalCase。

方法命名：

```python
class User:
    let name: String

    fn get_name(self) -> String:
        return self.name
```

规则：

- 方法名必须使用下划线风格，也就是 snake_case。
- 方法名只能使用小写字母、数字和下划线。
- 方法名不能以数字开头。
- 方法名不能包含大写字母。
- 单个小写单词也视为合法 snake_case，例如 `speak`、`init`。
- 内置方法也遵守该规则，例如 `to_string()`、`type_name()`、`is_empty()`。

非法示例：

```python
class User:
    fn getName(self):      # 错误：驼峰命名
        return self.name

    fn GetName(self):      # 错误：首字母大写
        return self.name
```

普通变量和类属性第一版不强制命名风格，但推荐使用 snake_case。

`final` 不按 `const` 常量规则强制大写。它表达“一次赋值”，不一定表达“全局常量”。例如类属性 `final owner_id: String` 是合法的。

## 5. 内置类型方法

Icoo 中所有数据类型都应支持内置方法调用。也就是说，基础类型不是只能通过全局函数操作，也可以像对象一样调用方法。

示例：

```python
let age = 18
let name = "Tom"
let values = [1, 2, 3]
let scores = {"Tom": 95, "Lucy": 88}

print(age.to_string())
print(name.len())
print(values.len())
print(scores.has("Tom"))
print(nil.to_string())
```

所有类型都必须支持的基础方法：

```text
to_string() -> String
type_name() -> String
```

各类型推荐方法：

```text
Nil
  to_string() -> String      # "nil"
  type_name() -> String      # "Nil"

Bool
  to_string() -> String      # "true" 或 "false"
  type_name() -> String
  to_int() -> Int            # true 为 1，false 为 0

Int
  to_string() -> String
  type_name() -> String
  to_float() -> Float
  abs() -> Int

Float
  to_string() -> String
  type_name() -> String
  to_int() -> Int
  abs() -> Float

String
  to_string() -> String
  type_name() -> String
  len() -> Int
  is_empty() -> Bool
  contains(value: String) -> Bool

Array
  to_string() -> String
  type_name() -> String
  len() -> Int
  is_empty() -> Bool

  # 参考 JavaScript Array 的基础变更方法
  push(value: Any) -> Nil
  pop() -> Any
  shift() -> Any
  unshift(value: Any) -> Int

  # 参考 JavaScript Array 的查询和派生方法
  at(index: Int) -> Any
  includes(value: Any) -> Bool
  index_of(value: Any) -> Int
  slice(start: Int, end: Int?) -> Array
  splice(start: Int, deleteCount: Int, items: Any...) -> Array
  join(separator: String = ",") -> String
  reverse() -> Array

  # 依赖函数值，建议在函数系统稳定后实现
  for_each(callback: Function) -> Nil
  map(callback: Function) -> Array
  filter(callback: Function) -> Array
  reduce(callback: Function, initial: Any?) -> Any
  find(callback: Function) -> Any
  find_index(callback: Function) -> Int
  some(callback: Function) -> Bool
  every(callback: Function) -> Bool

Map
  to_string() -> String
  type_name() -> String
  len() -> Int
  is_empty() -> Bool

  # 参考 JavaScript Map
  size() -> Int
  has(key: String) -> Bool
  get(key: String) -> Any
  set(key: String, value: Any) -> Map
  delete(key: String) -> Bool
  clear() -> Nil
  keys() -> Array
  values() -> Array
  entries() -> Array
  for_each(callback: Function) -> Nil

Function
  to_string() -> String
  type_name() -> String

Coroutine
  to_string() -> String
  type_name() -> String

Task
  to_string() -> String
  type_name() -> String
  is_done() -> Bool
  is_failed() -> Bool
  result() -> Any
  cancel() -> Nil

EventLoop
  to_string() -> String
  type_name() -> String
  spawn(coroutine: Coroutine) -> Task
  run() -> Nil
  run_until(task: Task) -> Any
  stop() -> Nil

Class
  to_string() -> String
  type_name() -> String

Instance
  to_string() -> String
  type_name() -> String
```

用户类实例默认继承 `to_string()` 和 `type_name()` 行为：

```python
class User:
    let name: String

    fn init(self, name: String):
        self.name = name

let user = User("Tom")
print(user.to_string())   # 默认类似 "<User instance>"
print(user.type_name())   # "User"
```

用户可以在类中定义同名方法覆盖默认行为：

```python
class User:
    let name: String

    fn init(self, name: String):
        self.name = name

    fn to_string(self) -> String:
        return "User(" + self.name + ")"
```

内置方法规则：

- 方法调用语法统一使用 `value.method(args...)`。
- `Array` 和 `Map` 的集合方法参考 JavaScript 语义，但命名遵守 Icoo 的 snake_case 规则，例如 `index_of`、`for_each`、`find_index`。
- `len()` 是 Icoo 为所有可计数类型提供的统一方法；对 `Map` 来说，`len()` 与 `size()` 等价。
- 如果值是用户类实例，优先查找实例类中定义的方法。
- 如果用户类没有定义该方法，再查找所有实例共有的默认内置方法。
- 如果值是基础类型，则查找该类型对应的内置方法表。
- 找不到方法时，报运行时错误。
- `to_string()` 是语言级通用转换方法，标准库函数 `str(value)` 应直接调用该方法。

## 6. 词法结构

Icoo 使用缩进定义代码块。

主要 Token：

```text
IDENT
NUMBER
STRING
NEWLINE
INDENT
DEDENT
EOF

+ - * / %
= == !=
< <= > >=
<-          继承箭头
. , : ( ) [ ] { }
```

关键字：

```text
let
const
final
fn
class
if
elif
else
while
for
in
return
break
continue
true
false
nil
self
super
and
or
not
import
from
as
async
co
yield
await
```

词法器处理 `<` 时，应优先匹配最长 token：

```text
<-  LeftArrow
<=  LessEqual
<   Less
```

## 7. 语法设计

顶层程序：

```ebnf
program        = statement* EOF ;

statement      = let_decl
               | const_decl
               | final_decl
               | fn_decl
               | coroutine_fn_decl
               | class_decl
               | if_stmt
               | while_stmt
               | return_stmt
               | yield_stmt
               | await_stmt
               | break_stmt
               | continue_stmt
               | expr_stmt ;

let_decl       = "let" IDENT type_hint? ("=" expression)? ;
const_decl     = "const" IDENT type_hint? "=" expression ;
final_decl     = "final" IDENT (type_hint ("=" expression)? | "=" expression) ;

type_hint      = ":" type_name ;
type_name      = IDENT ;

fn_decl        = "fn" IDENT "(" params? ")" return_type? ":" block ;
coroutine_fn_decl = "async" "fn" IDENT "(" params? ")" return_type? ":" block ;
params         = param ("," param)* ;
param          = IDENT type_hint? ;
return_type    = "->" type_name ;

class_decl     = "class" IDENT inheritance? ":" class_block ;
inheritance    = "<-" IDENT ;

class_block    = NEWLINE INDENT class_member* DEDENT ;
class_member   = field_decl | fn_decl | coroutine_fn_decl ;
field_decl     = let_field | const_field | final_field ;
let_field      = "let" IDENT type_hint ("=" expression)? ;
const_field    = "const" IDENT type_hint "=" expression ;
final_field    = "final" IDENT type_hint ("=" expression)? ;

block          = NEWLINE INDENT statement* DEDENT ;
yield_stmt     = "yield" expression? ;        // 兼容/底层能力，推荐用 await sleep(0)
await_stmt     = "await" expression ;
```

表达式：

```ebnf
expression     = assignment ;
assignment     = call_or_get "=" assignment | logic_or ;
logic_or       = logic_and ("or" logic_and)* ;
logic_and      = equality ("and" equality)* ;
equality       = comparison (("==" | "!=") comparison)* ;
comparison     = term ((">" | ">=" | "<" | "<=") term)* ;
term           = factor (("+" | "-") factor)* ;
factor         = unary (("*" | "/" | "%") unary)* ;
unary          = ("not" | "-" | "await") unary | call_or_get ;
call_or_get    = primary (call | get)* ;
call           = "(" args? ")" ;
get            = "." IDENT ;
primary        = NUMBER
               | STRING
               | "true"
               | "false"
               | "nil"
               | IDENT
               | "self"
               | "super"
               | array_literal
               | map_literal
               | "(" expression ")" ;

array_literal  = "[" elements? "]" ;
elements       = expression ("," expression)* ","? ;

map_literal    = "{" map_entries? "}" ;
map_entries    = map_entry ("," map_entry)* ","? ;
map_entry      = STRING ":" expression ;
```

第一版 `Map` 字面量只支持字符串 key：

```python
let scores = {"Tom": 95, "Lucy": 88}
```

这是为了降低运行时实现复杂度。后续如果需要更贴近 JavaScript `Map`，可以把 key 扩展为任意可比较值。

## 8. 协程设计

Icoo 的协程直接以事件循环为核心设计。协程本身表示可暂停、可恢复的执行体，但用户代码不直接手动驱动协程；协程由 `EventLoop` 调度，用户通过 `Task` 句柄观察结果、等待完成或取消任务。

为避免语言运行时被单线程限制，`EventLoop` 是语言层抽象，不等同于一个固定 OS 线程。第一版应按“可多线程调度”的模型设计：

- 语言层采用 `async fn` + `await` 语义：每个异步任务通过 `await`、定时器或 I/O 等待点主动让出控制权。
- 运行时层可以把不同 `Task` 分发到多个 worker 线程执行，默认后端建议使用 Tokio multi-thread runtime。
- Icoo 源码不暴露 Tokio 类型；用户只接触 `async fn`、`await`、`EventLoop`、`Task` 和 `Coroutine`。
- `EventLoop` 负责维护任务表、就绪任务、等待源、定时器、取消状态和错误传播；这些结构可以由底层后端并发驱动。
- 不做抢占式时间片。CPU 密集型异步函数如果长期不 `await`，仍然会占住当前 worker，因此需要 `await sleep(0)` 或显式放入阻塞/计算任务池。
- 跨线程调度只允许发生在运行时认为安全的任务边界。捕获了不可跨线程状态的协程可以被限制在同一个本地任务集合中运行。

推荐实现分层：

```text
Icoo 源语言
  async fn / await / EventLoop / Task / Coroutine
        |
        v
RuntimeBackend trait
        |
        +-- TokioBackend（默认，多 worker 线程）
        +-- LocalBackend（测试或最小实现，可单线程）
```

因此，Tokio 可以作为默认实现依赖，但不应成为 Icoo 的语言特性。这样后续可以替换运行时后端，或在嵌入场景中使用自定义调度器。

推荐语法：

```python
async fn worker(name: String):
    print(f"{name}: start")
    await sleep(0)
    print(f"{name}: end")
    return name

let loop = EventLoop()
let a = loop.spawn(worker("A"))
let b = loop.spawn(worker("B"))

loop.run()

print(a.result())      # "A"
print(b.result())      # "B"
```

等待任务：

```python
async fn counter(limit: Int) -> Int:
    let i = 0
    while i < limit:
        await sleep(0)
        i = i + 1
    return i

async fn main():
    let loop = current_loop()
    let task = loop.spawn(counter(3))
    let value = await task
    print(value.to_string())

let loop = EventLoop()
loop.spawn(main())
loop.run()
```

核心规则：

- `async fn` 定义异步函数；实现层会把它编译为可恢复的 `Coroutine`。
- 调用协程函数不会立即执行函数体，而是返回 `Coroutine` 对象。
- `EventLoop.spawn(coroutine)` 把协程加入事件循环，返回 `Task`。
- `EventLoop.run()` 运行事件循环，直到没有可运行任务或被 `stop()` 停止。
- `EventLoop.run_until(task)` 运行事件循环，直到指定任务完成，并返回任务结果。
- `current_loop()` 只能在事件循环正在运行的协程内调用，返回当前任务所属事件循环。
- `await task` 暂停当前协程，直到目标 `Task` 完成；完成后表达式值为任务结果。
- `await sleep(0)` 是推荐的主动让出调度写法。
- `return expression` 结束协程，并把最终结果写入对应 `Task`。
- `yield` 作为兼容/底层语句保留，用户代码推荐不用；如果使用，只能出现在 `async fn` 内部。
- `await` 只能出现在 `async fn` 内部；顶层 `await` 第一版不支持。
- 协程可以作为普通值保存、传参、放入 `Array` 或 `Map`。
- 协程函数可以作为类方法。
- 同一个 `Coroutine` 只能被一个事件循环 `spawn` 一次；重复调度是运行时错误。

类方法示例：

```python
class Job:
    let name: String

    fn init(self, name: String):
        self.name = name

    async fn run(self):
        await sleep(0)
        print(self.name + ":start")
        await sleep(0)
        print(self.name + ":end")
        return "done"

let loop = EventLoop()
let task = loop.spawn(Job("build").run())
loop.run_until(task)
```

事件循环中的任务关系：

```python
async fn fetch_user(id: Int) -> String:
    await sleep(0)
    return f"user:{id}"

async fn handle_request():
    let loop = current_loop()
    let user_task = loop.spawn(fetch_user(7))
    let user = await user_task
    print(user)

let loop = EventLoop()
loop.spawn(handle_request())
loop.run()
```

协程状态：

```text
Created     已创建，尚未开始执行
Queued      已加入事件循环，等待运行
Running     正在执行，防止重入调度
Suspended   已执行到 await/yield，等待重新入队
Waiting     正在 await 某个任务、定时器或 I/O
Done        已 return 或运行结束
Failed      执行中发生错误
Cancelled   已取消
```

错误传播：

- 协程内部运行时错误会让协程进入 `Failed` 状态。
- 对应 `Task` 进入失败状态，并保存错误。
- `await failed_task` 会重新抛出该错误。
- `task.result()` 如果任务失败，也会抛出该错误。
- 如果事件循环结束时存在失败任务但无人 `await` 或读取 `result()`，第一版应打印未处理任务错误，避免静默失败。

调度模型：

- 事件循环维护逻辑上的就绪队列；多线程后端可以把就绪任务拆分到多个 worker 队列。
- `spawn()` 将新任务提交给运行时后端，返回语言层 `Task` 句柄。
- `await task` 将当前任务移入等待队列，并在目标任务完成后重新入队。
- `await sleep(0)` 将当前任务重新提交为可运行任务，后续可能由同一个 worker 或其他 worker 继续执行。
- `run()` 启动或进入后端运行时，直到就绪任务和等待源都为空，或被 `stop()` 停止。
- `sleep(ms)` 创建一个定时器任务，完成值为 `nil`，可以用 `await sleep(100)` 等待。
- 第一版没有真实异步 I/O 时，等待源主要是 `Task` 依赖和 `sleep(ms)` 定时器。

```python
async fn ticker(name: String):
    print(name + ":1")
    await sleep(0)
    print(name + ":2")

let loop = EventLoop()
loop.spawn(ticker("A"))
loop.spawn(ticker("B"))
loop.run()

# 输出顺序：
# A:1
# B:1
# A:2
# B:2
```

实现建议：

- AST 解释器阶段不要把 Rust 调用栈直接当作协程栈。
- 协程函数体应编译成可恢复状态机，或先在 AST 层维护 `CoroutineFrame`。
- 事件循环应拥有任务表和后端句柄，`Task` 只是任务 ID 的安全句柄，不直接暴露 Tokio `JoinHandle`。
- 为降低第一版复杂度，推荐把 `yield` 视为兼容语句；主要用户 API 是表达式级 `await`。
- 第一版 `await` 只等待 `Task`；后续再扩展到定时器、I/O Future、Channel。
- 如果使用 Tokio，`await sleep(ms)` 可映射到 `tokio::time::sleep()`；主动让步可用 `await sleep(0)` 或后续内置 `yield_now()`。
- CPU 密集型或阻塞型内置函数应通过专门 API 进入 blocking pool，避免阻塞 async worker。
- 后续如果引入字节码 VM，协程恢复点可以直接保存 instruction pointer，比 AST 状态机更干净。

## 9. 类属性声明

类实例属性必须在类体中显式声明。

```python
class User:
    let name: String
    let age: Int = 0
    const ROLE: String = "user"
    final user_id: String

    fn init(self, name: String, user_id: String):
        self.name = name
        self.user_id = user_id
```

属性规则：

- `let` 属性是可变实例属性。
- `const` 属性是实例常量属性，必须在声明时初始化，之后不能再赋值；命名也遵守 `const` 常量命名规则。
- `final` 属性是实例一次赋值属性，可以没有默认值，但只能被第一次赋值。
- 属性可以有默认值。
- 没有默认值的属性，必须在构造完成前赋值。这个规则主要适用于 `let` 和 `final` 属性。
- 给未声明属性赋值是错误。
- 子类继承父类所有属性。
- 第一版不允许子类声明与父类同名的属性。

非法示例：

```python
class User:
    let name: String

    fn init(self, name: String):
        self.name = name
        self.email = "x@test.com"  # 错误：email 未声明
```

实例构造流程：

1. 收集父类链上的字段。
2. 收集当前类声明的字段。
3. 创建包含所有声明字段的实例。
4. 执行字段默认值初始化。
5. 如果存在 `init` 方法，则调用 `init`。
6. 检查所有必填字段是否都已初始化。

## 10. 继承

Icoo 支持单继承。

```python
class Dog <- Animal:
    let breed: String
```

规则：

- 一个类最多只能有一个父类。
- 子类继承父类方法。
- 子类继承父类属性。
- 子类方法可以覆盖父类方法。
- 第一版子类属性不能重定义父类属性。
- `super.method(...)` 用于调用父类方法。

方法查找顺序：

1. 实例字段。
2. 当前类方法。
3. 父类方法。
4. 找不到则报运行时错误。

## 11. 运行时模型

推荐的 Rust 值表示：

```rust
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Map(std::collections::HashMap<String, Value>),
    Function(std::rc::Rc<Function>),
    Coroutine(std::rc::Rc<std::cell::RefCell<Coroutine>>),
    Task(TaskId),
    EventLoop(std::rc::Rc<std::cell::RefCell<EventLoop>>),
    Class(std::rc::Rc<Class>),
    Instance(std::rc::Rc<std::cell::RefCell<Instance>>),
}
```

上面的 `Rc<RefCell<...>>` 适合最小 AST 解释器或 `LocalBackend`。如果启用 Tokio 多线程后端，运行时不能直接把这些值跨 worker 移动。需要在设计上二选一：

- 共享并发模型：把可跨任务共享的运行时对象改为 `Arc<Mutex<T>>` 或 `Arc<RwLock<T>>`，并明确锁粒度，避免解释器执行期间长时间持锁。
- 任务隔离模型：每个协程拥有自己的 `CoroutineFrame` 和局部环境；跨任务通信只通过 `Task` 结果、Channel 或显式可发送值完成。用户实例默认不跨线程共享。

推荐第一版采用“任务隔离优先，少量共享对象显式封装”的策略。这样可以使用 Tokio multi-thread runtime 避免单线程瓶颈，同时不把整个对象系统过早改成高锁竞争结构。

多线程后端下建议增加运行时安全标记：

```rust
pub enum ShareMode {
    LocalOnly,   // 只能在创建它的本地任务集合中使用
    Sendable,    // 可以移动到其他 worker
    Shared,      // 内部带同步保护，可以多任务共享
}
```

默认规则：

- `Nil`、`Bool`、`Int`、`Float`、不可变 `String` 可以跨线程移动。
- `Array`、`Map` 只有在元素和值全部可发送时才可作为任务结果跨线程传递。
- `Function`、`Class` 可以作为不可变元数据共享，但闭包捕获的环境必须满足可发送规则。
- `Instance` 默认 `LocalOnly`；后续可以增加显式并发安全类或 actor 风格对象。
- `CoroutineFrame` 不共享，只能由当前正在执行该任务的 worker 拥有。

协程运行时结构：

```rust
pub struct Coroutine {
    pub name: String,
    pub state: CoroutineState,
    pub frame: CoroutineFrame,
    pub owner_task: Option<TaskId>,
}

pub enum CoroutineState {
    Created,
    Queued,
    Running,
    Suspended,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

pub struct CoroutineFrame {
    pub env: EnvRef,
    pub body: Vec<Stmt>,
    pub instruction: usize,
    pub stack: Vec<Value>,
    pub waiting_on: Option<WaitSource>,
}
```

事件循环结构：

```rust
pub type TaskId = u64;

pub struct EventLoop {
    pub next_task_id: TaskId,
    pub tasks: std::collections::HashMap<TaskId, Task>,
    pub ready: std::collections::VecDeque<TaskId>,
    pub waiting: std::collections::HashMap<TaskId, WaitSource>,
    pub stopped: bool,
}

pub struct Task {
    pub id: TaskId,
    pub coroutine: std::rc::Rc<std::cell::RefCell<Coroutine>>,
    pub state: TaskState,
    pub result: Option<Value>,
    pub error: Option<RuntimeError>,
    pub awaiters: Vec<TaskId>,
}

pub enum TaskState {
    Queued,
    Running,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

pub enum WaitSource {
    Task(TaskId),
    Timer(TimerId),
    Io(IoToken),
}
```

运行时后端抽象：

```rust
pub trait RuntimeBackend {
    fn spawn(&self, task: RunnableTask) -> BackendTaskHandle;
    fn yield_now(&self) -> BackendYield;
    fn sleep(&self, millis: u64) -> BackendSleep;
    fn block_on(&self, task: TaskId) -> Result<Value, RuntimeError>;
    fn shutdown(&self);
}

pub struct TokioBackend {
    pub runtime: tokio::runtime::Runtime,
    pub worker_threads: usize,
}
```

`EventLoop` 持有 `RuntimeBackend`，而不是直接持有 Tokio 运行时类型。`Task` 保存语言层状态和结果；后端句柄只负责唤醒、取消和运行 Rust future。这样 `EventLoop` 的公开行为稳定，底层可以从本地后端切换到 Tokio 多线程后端。

如果继续使用 AST 解释器，`CoroutineFrame` 至少需要保存：

- 当前执行到的语句位置。
- 当前局部变量环境。
- 当前块嵌套栈。
- `while` 等控制流的恢复位置。
- 当前等待源，例如任务、定时器或 I/O。

这会让 AST 解释器复杂度明显上升。因此协程是推动后续字节码 VM 的重要理由之一。

类元数据：

```rust
pub struct Class {
    pub name: String,
    pub superclass: Option<std::rc::Rc<Class>>,
    pub fields: Vec<FieldDef>,
    pub methods: std::collections::HashMap<String, Function>,
}

pub struct FieldDef {
    pub name: String,
    pub kind: FieldKind,
    pub type_hint: TypeRef,
    pub initializer: Option<Expr>,
}

pub enum FieldKind {
    Mutable,       // let
    Const,         // const，声明时初始化后不可修改
    Final,         // final，第一次赋值后不可修改
}
```

实例状态：

```rust
pub struct Instance {
    pub class: std::rc::Rc<Class>,
    pub fields: std::collections::HashMap<String, FieldValue>,
}

pub struct FieldValue {
    pub value: Value,
    pub initialized: bool,
    pub kind: FieldKind,
}
```

局部变量和全局变量也应使用类似的绑定策略：

```rust
pub enum BindingKind {
    Mutable,       // let
    Const,         // const，声明时必须初始化
    Final,         // final，可延迟初始化，但只能赋值一次
}

pub struct Binding {
    pub value: Value,
    pub initialized: bool,
    pub kind: BindingKind,
}
```

函数应捕获定义时的环境，以支持闭包。

方法本质上是函数。从实例访问方法时，解释器需要自动绑定 `self`。

基础类型方法可以通过“内置类型方法表”实现，而不需要把 `Int`、`String` 等全部包装成用户类实例。

推荐设计：

```rust
pub type NativeMethod = fn(receiver: Value, args: Vec<Value>) -> RuntimeResult<Value>;

pub struct BuiltinMethods {
    pub nil_methods: MethodTable,
    pub bool_methods: MethodTable,
    pub int_methods: MethodTable,
    pub float_methods: MethodTable,
    pub string_methods: MethodTable,
    pub array_methods: MethodTable,
    pub map_methods: MethodTable,
    pub function_methods: MethodTable,
    pub class_methods: MethodTable,
    pub instance_default_methods: MethodTable,
}

pub type MethodTable = std::collections::HashMap<String, NativeMethod>;
```

属性访问 `value.name` 的处理顺序：

1. 如果 `value` 是用户类实例，先查找实例字段。
2. 如果 `value` 是用户类实例，再查找类方法和父类方法。
3. 如果 `value` 是用户类实例且仍未找到，查找实例默认内置方法。
4. 如果 `value` 是基础类型，查找该类型的内置方法表。
5. 找不到则报运行时错误。

## 12. AST 结构

核心语句节点：

```rust
pub enum Stmt {
    Let(LetDecl),
    Const(ConstDecl),
    Final(FinalDecl),
    Function(FunctionDecl),
    CoroutineFunction(FunctionDecl),
    Class(ClassDecl),
    If(IfStmt),
    While(WhileStmt),
    Return(Option<Expr>),
    Yield(Option<Expr>),
    Break,
    Continue,
    Expr(Expr),
}

pub struct ClassDecl {
    pub name: Identifier,
    pub superclass: Option<Identifier>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FunctionDecl>,
}

pub struct FieldDecl {
    pub kind: FieldKind,
    pub name: Identifier,
    pub type_hint: TypeRef,
    pub initializer: Option<Expr>,
}

pub struct FunctionDecl {
    pub name: Identifier,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Vec<Stmt>,
    pub is_coroutine: bool,
}
```

## 13. 解释器架构

推荐项目结构：

```text
src/
  main.rs
  lib.rs

  lexer/
    mod.rs
    token.rs

  parser/
    mod.rs
    ast.rs

  resolver/
    mod.rs

  typecheck/
    mod.rs
    types.rs

  runtime/
    mod.rs
    value.rs
    env.rs
    function.rs
    class.rs
    coroutine.rs
    builtin_methods.rs
    native.rs

  interpreter/
    mod.rs

  error/
    mod.rs
```

执行流程：

```text
source code
  -> lexer
  -> tokens
  -> parser
  -> AST
  -> resolver
  -> optional type checker
  -> interpreter
  -> runtime values
```

Resolver 负责：

- 解析局部变量引用。
- 处理闭包捕获。
- 检测非法 `self`。
- 检测非法 `super`。
- 校验类继承引用。
- 校验类属性声明冲突。
- 校验命名规范，例如常量名、类名、方法名。
- 校验 `const` 是否声明时初始化。
- 校验 `final` 是否存在非法重复赋值。第一版可以在运行时做完整检查，Resolver 只做明显的声明错误检查。
- 校验 `await` 只能出现在 `async fn` 内部。
- 校验 `yield` 如果启用，只能出现在 `async fn` 内部。
- 校验协程函数和普通函数的返回、暂停语义边界。

## 14. 错误处理

错误分类：

```text
LexerError
ParseError
ResolveError
TypeError
RuntimeError
CoroutineError
```

每个错误应包含：

```text
file
line
column
message
```

示例：

```text
example.icoo:12:9: cannot assign undeclared field 'email' on class 'User'
example.icoo:3:7: constant name 'max_count' must use uppercase letters, digits, or '_'
example.icoo:8:7: class name 'user' must start with an uppercase letter
example.icoo:15:8: method name 'getName' must use snake_case
example.icoo:21:1: final binding 'runtime_id' can only be assigned once
example.icoo:28:14: final field 'owner_id' can only be assigned once
example.icoo:34:5: yield can only be used inside async functions
example.icoo:40:1: coroutine 'counter' has already been spawned
example.icoo:45:9: await can only be used inside async functions
example.icoo:52:1: task 3 failed: division by zero
```

Token 源码位置：

```rust
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}
```

## 15. 标准库 MVP

第一版内置函数：

```text
print(value)
len(value)
str(value)
int(value)
float(value)
type(value)
is_coroutine(value)
is_task(value)
current_loop()
sleep(ms: Int) -> Task
```

事件循环对象方法：

```text
loop.spawn(coroutine)
loop.run()
loop.run_until(task)
loop.stop()
loop.is_stopped()
```

任务对象方法：

```text
task.is_done()
task.is_failed()
task.result()
task.cancel()
```

这些函数应在执行用户代码前注册到全局环境中。

标准库函数和内置方法应保持一致：

- `str(value)` 等价于 `value.to_string()`。
- `type(value)` 等价于 `value.type_name()`。
- `len(value)` 优先调用 `value.len()`，如果该类型没有 `len()`，则报错。
- `Map.size()` 与 `Map.len()` 等价；保留 `size()` 是为了贴近 JavaScript Map。

如果同时提供全局函数和方法，方法是主要风格，全局函数是便利入口。

## 16. 实现阶段

阶段 1：Lexer

- Token 定义。
- 关键字识别。
- `final` 关键字。
- `async` 和 `await` 关键字。
- `co` 和 `yield` 作为兼容/底层关键字保留。
- 数字和字符串。
- 缩进处理。
- `INDENT` 和 `DEDENT`。
- `<-` 继承 token。

阶段 2：Parser

- 语句解析。
- 表达式解析。
- 函数声明。
- `async fn` 异步函数声明。
- `yield` 兼容语句。
- 类声明。
- 显式类属性声明。
- `final` 局部声明和 `final` 类属性声明。
- 保留声明名称的源码位置，方便 Resolver 输出命名错误。

阶段 3：解释器基础能力

- 表达式执行。
- 变量。
- 常量。
- `final` 一次赋值绑定。
- 块作用域。
- 内置 `print`。
- 基础类型的 `to_string()` 和 `type_name()`。

阶段 4：函数

- 函数调用。
- `return`。
- 闭包。
- 参数数量检查。

阶段 4.5：协程 MVP

- `async fn` 解析。
- `co fn` 作为兼容别名解析。
- `yield` 兼容语句解析。
- `await` 表达式解析。
- `Coroutine` 运行时值。
- `Task` 运行时值。
- `EventLoop` 运行时值。
- `EventLoop.spawn()`、`EventLoop.run()`、`EventLoop.run_until()`。
- `Task.is_done()`、`Task.result()`、`Task.cancel()`。
- `await sleep(0)` 主动让出调度。
- 语句级 `yield` 作为兼容能力。
- `await task` 等待任务完成。
- 协程状态：Created、Queued、Running、Suspended、Waiting、Done、Failed、Cancelled。
- 事件循环就绪队列和等待队列。
- Resolver 校验 `yield` 位置。
- Resolver 校验 `await` 位置。

阶段 4.6：运行时后端抽象

- 定义 `RuntimeBackend` trait。
- 实现 `LocalBackend`，用于单元测试和最小解释器验证。
- 实现 `TokioBackend`，默认使用 Tokio multi-thread runtime。
- `EventLoop` 只依赖 `RuntimeBackend`，不向 Icoo 语言层暴露 Tokio 类型。
- `Task` 包装后端任务句柄，保留语言层任务状态、结果、错误和取消语义。
- 明确 `Value` 的跨线程规则：可发送值、共享值、本地值。
- 对 `Instance`、闭包环境、`CoroutineFrame` 做任务隔离，避免未同步状态跨 worker 共享。
- 为 CPU 密集型或阻塞型内置函数预留 blocking pool 接口。

阶段 5：类

- 类对象。
- 实例创建。
- 字段初始化。
- 方法绑定。
- 声明字段的赋值检查。
- `final` 字段第一次赋值检查。
- 用户实例默认内置方法。

阶段 6：继承

- `class Child <- Parent`。
- 沿父类链查找方法。
- `super.method(...)`。
- 继承字段。

阶段 7：类型检查

- 基于类型标注的运行时检查。
- 基于明显静态类型的提前检查。
- 函数参数检查。
- 函数返回值检查。
- 类属性赋值检查。
- 内置方法参数检查，例如 `String.contains(value: String)`、`Array.join(separator: String)`、`Array.slice(start: Int, end: Int?)`、`Map.get(key: String)`、`Map.set(key: String, value: Any)`。

阶段 8：内置类型方法完善

- `String.len()`、`String.contains()`。
- `Array.push()`、`Array.pop()`、`Array.shift()`、`Array.unshift()`。
- `Array.includes()`、`Array.index_of()`、`Array.slice()`、`Array.splice()`、`Array.join()`。
- `Array.map()`、`Array.filter()`、`Array.reduce()` 等依赖函数回调的方法。
- `Map.size()`、`Map.has()`、`Map.get()`、`Map.set()`、`Map.delete()`、`Map.clear()`。
- `Map.keys()`、`Map.values()`、`Map.entries()`、`Map.for_each()`。
- 全局函数与内置方法保持一致。

阶段 9：命名规范校验

- `const` 名称只能使用大写字母、数字和 `_`。
- 类名首字母必须大写。
- 方法名必须使用 snake_case。
- 输出带源码位置的 `ResolveError`。

## 17. 架构决策

决策：第一版使用 AST 解释器。

原因：

- 语言对象模型和语法仍在演进。
- AST 解释器更容易调试。
- 类属性、继承、闭包、`super` 等语义先直接实现更稳妥。
- 内置类型方法可以先用运行时方法表实现，不需要一开始引入完整对象元类系统。
- 命名规范放在 Resolver 中统一校验，避免 Lexer 和 Parser 承担过多语义职责。
- 协程直接基于事件循环设计，用户通过 `EventLoop` 和 `Task` 组织并发流程。
- 协程 MVP 以 `async fn` 和 `await task` 为主要用户语法，`yield` 仅作为兼容能力，避免 AST 解释器过早承担完整异步 I/O 复杂度。
- `EventLoop` 是语言层抽象，默认运行时后端采用 Tokio multi-thread runtime，避免实现被单线程调度限制。
- Tokio 只作为 Rust 实现细节，不进入 Icoo 源码语法和标准库公开类型。
- 多线程后端采用任务隔离优先策略，只有满足发送或同步规则的值才能跨 worker 传递。
- 后续可以在不改变源语言的情况下增加字节码后端。

第一版不采用：

- 直接字节码编译器。
- 纯静态类型系统。
- 动态未声明实例字段。
- 多继承。
- 抢占式协程调度。
- 暴露 Tokio API 到 Icoo 源语言。
- 任意对象的隐式跨线程共享。
- 完整异步 I/O 标准库。

ADR：默认使用 Tokio 作为事件循环后端。

背景：

- 用户需要避免单线程运行时瓶颈。
- Rust 生态中 Tokio 的定时器、任务调度、I/O 和 blocking pool 成熟度高。
- Icoo 语言层希望保持简单，不把具体 Rust 异步库变成语法概念。

决策：

- `EventLoop` 通过 `RuntimeBackend` 驱动。
- 默认后端为 `TokioBackend`，使用 Tokio multi-thread runtime。
- 保留 `LocalBackend` 作为测试、嵌入和最小实现后端。
- `Task` 包装后端句柄，用户不能直接操作 Tokio `JoinHandle`。

影响：

- 可以利用多 worker 线程执行不同任务，降低单线程事件循环瓶颈。
- AST 解释器需要更清晰的任务边界和值发送规则。
- 运行时对象不能继续无条件使用 `Rc<RefCell<...>>` 跨任务共享；多线程路径需要 `Arc`、锁、不可变共享或任务隔离。
- 后续增加真实异步 I/O 时，可以直接接入 Tokio 的文件、网络、定时器能力。

## 18. 待确认问题

实现前或第一轮实现过程中需要确认：

- 类属性类型标注是否必须始终填写？
- `Map` 的 key 是否长期限制为 `String`，还是后续扩展为任意可比较值？
- 顶层代码是否允许 `return`，还是只允许在函数中使用？
- 子类有父类必填字段时，是否强制调用 `super.init(...)`？
- `to_string()` 输出格式是否需要稳定为可解析格式，还是仅用于调试和展示？
- 常量名的“特殊符号”是否只保留 `_`，还是需要额外支持 `$` 等符号？
- 第一版是否允许表达式级 `yield`，例如 `let x = yield value`？
- `EventLoop.run()` 遇到未处理失败任务时，是立即中止还是收集所有错误后统一报告？
- 第一版 `sleep(ms)` 使用真实定时器，还是先作为测试用的调度让步？
- 后续是否需要全局默认事件循环，还是所有协程都必须显式绑定到 `EventLoop`？
- `EventLoop()` 默认 worker 线程数量是否使用 CPU 核心数，还是允许 `EventLoop(workers: 4)` 显式配置？
- 用户类是否需要显式声明为可跨线程共享，还是第一版全部 `Instance` 都限制为任务本地对象？
