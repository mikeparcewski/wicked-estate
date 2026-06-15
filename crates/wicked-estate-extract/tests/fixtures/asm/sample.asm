section .data
    msg db 'Hello, World!', 10
    msg_len equ $ - msg

section .bss
    buf resb 64

section .text
    global _start
    global add
    global factorial

add:
    push rbp
    mov  rbp, rsp
    mov  eax, edi
    add  eax, esi
    pop  rbp
    ret

factorial:
    push rbp
    mov  rbp, rsp
    cmp  edi, 1
    jle  .base
    mov  eax, edi
    dec  edi
    call factorial
    imul eax, [rbp - 4]
    jmp  .done
.base:
    mov  eax, 1
.done:
    pop  rbp
    ret

_start:
    mov  rax, 1
    mov  rdi, 1
    mov  rsi, msg
    mov  rdx, msg_len
    syscall
    mov  rax, 60
    xor  rdi, rdi
    syscall
