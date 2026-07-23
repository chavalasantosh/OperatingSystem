.intel_syntax noprefix
.section .text
.global _start
_start:
    mov rax, 0
    lea rdi, [rip + message]
    mov rsi, 43
    syscall
    movabs rbx, 0x0000600000000000
    mov rax, qword ptr [rbx]
    mov rax, 2
    xor rdi, rdi
    syscall
    ud2
.section .rodata
message:
    .ascii "fault-test: requesting isolated page fault\n"
.set message_len, . - message
