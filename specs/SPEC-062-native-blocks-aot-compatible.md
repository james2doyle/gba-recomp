# SPEC-062 — Blocos nativos LLVM sobre o núcleo `gba-core` (`aot-compatible`)

Estado do documento: Accepted em 2026-10-14. Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

Com o núcleo interpretado completo (SPEC-056–060), a SPEC-055 pode ser cumprida: gerar, por ROM, código nativo LLVM para as instruções guest alcançadas e executá-lo sobre o mesmo estado e scheduler do núcleo, usando o interpretador apenas como shell explícito de um passo. O resultado é um executável por jogo com rótulo `aot-compatible`.

## Fora de escopo

JIT (proibido), código em RAM nativo (etapa posterior com guards IAC-08; nesta etapa todo código fora da ROM é interpretado e contado), alteração do modo `strict-aot`.

## Termos

- **Run**: sequência linear de instruções de um modo (ARM/Thumb) emitida como uma função LLVM `u32 f(ctx, entry)`; termina num branch, escrita de PC, shell ou fim da cobertura.
- **Entrada de retomada**: cada instrução de um run tem índice próprio; o dispatcher pode entrar em qualquer uma.
- **Shell**: saída `EXIT_INTERPRET` que pede ao núcleo para interpretar exatamente uma instrução (IAC-05/06).

## Requisitos normativos

- NB-62-01 (equivalência): para o mesmo estado inicial e entradas, executar um run nativo MUST produzir os mesmos registradores, CPSR, memória, eventos e **ciclos guest** que o interpretador da SPEC-056 instrução a instrução. Custos de fetch usam a tabela de waitstates corrente do bus (inclusive prefetch); acessos de dados passam por helpers que cobram os mesmos ciclos; ciclos internos e o estado sequencial/não sequencial seguem as mesmas regras do interpretador.
- NB-62-02 (eventos): antes de cada instrução, exceto a primeira de uma chamada, o código MUST sair com `EXIT_SEQ` se `cycles ≥ next_event`. Após qualquer store em I/O (região 0x04), o run MUST completar a instrução e sair. Assim IRQ, DMA, HALT e eventos são tratados na mesma fronteira de instrução que no interpretador.
- NB-62-03 (shells): MRS/MSR, SWI, encodings indefinidos/coprocessador, data processing com `Rd=PC` e `S=1`, LDM/STM com `^`, SWP e writeback com base PC MUST sair com `EXIT_INTERPRET` sem efeitos. Thumb SWI e indefinidos idem.
- NB-62-04 (branches): B/BL/BX, escritas de PC por ALU/LDR/LDM/POP/hi-reg MUST sair com `EXIT_BRANCH` e PC destino alinhado ao modo; o runtime aplica o refill do pipeline (N+S) exatamente como o interpretador.
- NB-62-05 (ABI): o contexto nativo `GbaNativeCtxV1` (`#[repr(C)]`) contém r0–r15, CPSR, flag sequencial, ciclos, próximo evento, contador de instruções nativas, último opcode (open bus), ponteiro para tabela de waitstates e ponteiro opaco do bus. Endereços guest são `u32`; o código gerado nunca recebe ponteiros para memória guest — todo acesso passa pelos helpers `gba_rt_*` (regra 7 do AGENTS).
- NB-62-06 (descoberta): entradas vêm de (a) execução interpretada de perfil, identificada por SHA-256 da ROM, script de input e número de frames (IAC-04), e (b) alvos diretos de branch alcançáveis estaticamente a partir delas. Somente endereços de ROM são compilados nesta etapa.
- NB-62-07 (pacote): `gba-recomp compile --mode aot-compatible --profile-frames N [--profile-input …]` gera diretório com executável único (objeto LLVM + runtime `gba-native-rt` estático) e manifesto com `execution_mode: aot-compatible`, versões de núcleo/HLE/ABI nativa, ROM SHA-256, número de runs/instruções nativas, fonte do perfil e limites (IAC-09). Falha de geração não deixa pacote parcial.
- NB-62-08 (métricas): o runner MUST reportar instruções nativas, instruções interpretadas (fora de cobertura + shells), shells, misses do dispatcher e saídas por evento/I/O, com a janela em frames (IAC-10).
- NB-62-09 (diferencial): `--verify-interpreter` MUST executar o mesmo cenário puramente interpretado e comparar hash de framebuffer, ciclos e registradores por frame; divergência é falha (IAC-11). A igualdade valida a tradução, não a fidelidade de hardware.

## Interfaces

```c
typedef struct GbaNativeCtxV1 {
  uint32_t r[16]; uint32_t cpsr; uint32_t next_seq;
  uint64_t cycles; uint64_t next_event; uint64_t retired;
  uint32_t last_opcode; uint32_t reserved;
  const uint8_t *wait;   /* n16[16] s16[16] n32[16] s32[16] prefetch */
  void *bus;
} GbaNativeCtxV1;
uint32_t run(GbaNativeCtxV1 *ctx, uint32_t entry); /* 0 SEQ, 1 BRANCH, 2 INTERPRET */
```

Tabela exportada pelo objeto: `gba_aot_entries[] = {pc|thumb, index, fn}`, `gba_aot_entry_count`, `gba_aot_rom_sha256[32]`.

## Critérios de aceite

Testes de lowering por família comparando nativo × interpretado em programas sintéticos; diferencial `--verify-interpreter` na LeafGreen por N frames sem divergência em O0 e O2; manifesto e métricas presentes; rejeição de ROM com SHA diferente no runner.

## Referências

[SPEC-055](SPEC-055-interpreter-assisted-compatibility.md), [SPEC-056](SPEC-056-armv4t-interpreter.md), [SPEC-057](SPEC-057-bus-scheduler-io.md), [ADR-040](../adr/ADR-040-interpreter-assisted-compatibility.md), [ADR-041](../adr/ADR-041-interpreter-first-core.md).
