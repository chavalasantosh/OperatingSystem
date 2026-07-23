.intel_syntax noprefix
.section .text
.global _start
_start:
    mov rax, 0
    lea rdi, [rip + message]
    mov rsi, 41
    syscall
    mov rax, 4
    syscall
    mov r12, 5000000
1:
    pause
    dec r12
    jnz 1b
    mov rax, 3
    syscall
    mov rax, 2
    xor rdi, rdi
    syscall
    ud2
.section .rodata
message:
    .ascii "init: SanjuOS protected userspace online\n"
.set message_len, . - message
