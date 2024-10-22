# 实验报告

## 实现功能

在本章节实验中，我实现了一个新的系统调用 `sys_task_info`，其功能主要是获取当前正在执行的任务的相关信息。

实现步骤大致如下：
1. 在加载任务时，除了已有的 task-context, task-status 信息，还另外新增一个 task-info 数据，用于记录任务的相关计数
2. 在 handle syscall 时，根据 syscall id 增加相应的计数信息
3. 增加系统调用 `sys_task_info`，当被调用时读取相应的 task-info 信息并返回

根据以上步骤，可以在进程进行syscall时记录相关信息，并可以通过 `sys_task_info` 读取到当前进程的相应信息。

## 简答作业

1. 正确进入 U 态后，程序的特征还应有：使用 S 态特权指令，访问 S 态寄存器后会报错。 请同学们可以自行测试这些内容（运行 三个 bad 测例 (ch2b_bad_*.rs) ）， 描述程序出错行为，同时注意注明你使用的 sbi 及其版本。

> 答：
> 
> 在运行三个bad测试用例的时候，表现和原因如下：
> 1. ch2b_bad_address：程序试图向一个受保护的内存地址写入数据，触发 StoreFault 异常
> 2. ch2b_bad_instructions：程序在 U 态使用 S 态特权指令，触发 IllegalInstruction 异常
> 3. ch2b_bad_register：程序在 U 态试图读取 S 态寄存器，触发 IllegalInstruction 异常
>
>    在进行实验时，我使用的 sbi 为: RustSBI version 0.3.0-alpha.2

2. 深入理解 trap.S 中两个函数 __alltraps 和 __restore 的作用，并回答如下问题:
    1. L40：刚进入 __restore 时，a0 代表了什么值。请指出 __restore 的两种使用情景。
    > 答：
    > 
    > 在进入 __restore 时，a0 代表了当前任务的 TrapContext。
    > 
    > __restore 使用场景：
    > 1. 内核使用 __restore 启动任务。
    > 2. 当一个进程触发 trap 后，内核使用 __restore 恢复当前任务状态

    
    2. L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释。
    ```asm
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    ld t2, 2*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    csrw sscratch, t2
    ```
    > 答：
    >
    > 这几行分别从 TrapContext 的 sstatus, sepc, x2中读取值并写入到寄存器 sstatus, sepc, sscratch 中。
    > 在进入用户态时，几个寄存器的作用分别为：
    > * sstatus: 记录了trap发生之前的特权级等信息，以便在sret是进入到正确的特权级
    > * sepc: 存储用户态触发trap时的地址，在sret时可以让pc跳转到正确的位置
    > * sscratch: 临时存储用户态sp的值；在进入用户态之前读出到sp寄存器并机器内核态sp

    3. L50-L56：为何跳过了 x2 和 x4？
    ```asm
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
    LOAD_GP %n
    .set n, n+1
    .endr
    ```
    > 答：
    >
    > 1. x2实际上是sp寄存器，此时sp已指向正确的内存地址，故跳过
    > 2. x4是tp(thread pointer)寄存器，我们的应用还没用到，故跳过

    4. L60：该指令之后，sp 和 sscratch 中的值分别有什么意义？
    ```asm
    csrrw sp, sscratch, sp
    ```
    > 答：
    >
    > 在前面 L48 中，我们从 TrapContext 中读取了用户态 sp 到 sscratch。
    >
    >故在 L60 时，`csrrw sp, sscratch, sp` 交换了 sscratch 和 sp 寄存器的值后，sp指向用户态栈顶，sscratch指向内核态栈顶。
    >
    >这使得 sret 之后进入用户态时程序可以直接通过 sp 获取user stack的内容。

    5. __restore：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？
    > 答：
    >
    > __restore 在指令 `sret` 处发生状态切换。在执行此指令时，CPU会检查 sstatus 的值，由于我们在之前已经将 sstatus 的 SPP 位设置为0，故执行 `sret` 时会进入到用户态。

    6. L13：该指令之后，sp 和 sscratch 中的值分别有什么意义？
    ```asm
    csrrw sp, sscratch, sp
    ```
    > 答：
    >
    > L13位于 __alltraps 的头部，__alltraps由用户态程序触发，故执行到L13时sp指向user stack，sscratch指向kernel stack。
    >
    > L13的作用是交换sp和sscratch寄存器的值，故交换完后，sp寄存器指向kernel stack，sscratch寄存器指向user stack。
    >
    > 由于此时系统处于内核态，故sp指向kernel stack是很合理的，更便于我们进行编程。

    7. 从 U 态进入 S 态是哪一条指令发生的？
    > 答：
    >
    > 用户态(U 态)程序在调用 ecall 指令时，会 trap 到 S 态。

## 荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

> 未有交流的对象

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    > 我在编码过程中通过 `RISCV_CARD.pdf` 查阅RISC-V的寄存器和指令相关信息。由于这些信息是通用的，并非具体的直接帮助到我完成实验的内容，故未标注到代码中。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
