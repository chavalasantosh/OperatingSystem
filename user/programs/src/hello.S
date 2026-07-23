.intel_syntax noprefix
.section .text
.global _start
_start:
    mov rax, 0
    lea rdi, [rip + message]
    mov rsi, 35
    syscall
    mov rax, 4
    syscall
    mov rax, 3
    syscall
    mov rax, 2
    xor rdi, rdi
    syscall
    ud2
.section .rodata
message:
    .ascii "hello: running from SanjuOS Ring 3\n"
.set message_len, . - message
