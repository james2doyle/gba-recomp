# SPEC-056 — Interpretador ARM7TDMI completo do núcleo `gba-core`

Estado do documento: Accepted em 2026-10-14. Progresso: [ROADMAP](../ROADMAP.md).

## Problema, objetivos e escopo

A SPEC-055 exige um interpretador local completo antes de qualquer pacote `aot-compatible`, e a ADR-041 decide construir o núcleo GBA interpretado primeiro. Esta spec define a CPU: decodificação e execução de todo o ARMv4T (ARM e Thumb) do ARM7TDMI, exceções e bancos, sobre uma interface de bus abstrata. Os efeitos de memória e o tempo de acesso ficam na SPEC-057.

## Fora de escopo

ARMv5+ (BLX, CLZ, QADD, LDRD…), Thumb-2, coprocessadores (o GBA não tem), JIT, uso pelo modo strict-AOT, precisão de ciclo intra-instrução além do contrato de custo abaixo.

## Termos e invariantes

- Estado: r0–r15 visíveis, CPSR, bancos r8–r14 FIQ, r13–r14 de IRQ/SVC/ABT/UND, SPSR por modo privilegiado.
- Pipeline: leituras de PC veem `pc+8` (ARM) e `pc+4` (Thumb), exceto os casos listados em CPU-56-04.
- Passo: uma instrução aposentada ou uma entrada em exceção; nunca um NOP implícito.

## Requisitos normativos

- CPU-56-01: o decoder MUST classificar todas as famílias ARMv4T: data processing (imediato, shift imediato, shift por registrador), MRS/MSR (registrador e imediato, máscaras de campo), MUL/MLA, UMULL/UMLAL/SMULL/SMLAL, SWP/SWPB, LDR/STR/LDRB/STRB (pré/pós, writeback, offset imediato/registrador com shift), LDRH/STRH/LDRSB/LDRSH, LDM/STM (todos os modos, `^`), B/BL, BX, SWI, coprocessador e espaço indefinido; e os 19 formatos Thumb. Encoding indefinido ou de coprocessador MUST entrar na exceção Undefined (vetor 0x04), nunca virar NOP.
- CPU-56-02: ALU e shifter MUST seguir CPU-04/CPU-05: shift imediato 0 com LSR/ASR = 32, ROR #0 = RRX; shift por registrador usa os 8 bits baixos, valor 0 preserva carry, ≥32 conforme operação; operandos imediatos rotacionados atualizam C apenas quando a rotação é não nula. Operações lógicas atualizam C pelo shifter e preservam V.
- CPU-56-03: data processing com `Rd=PC` e `S=1` MUST copiar SPSR para CPSR (retorno de exceção); em User/System esse caso é imprevisível e MUST ser reportado como fault `unpredictable`, não inventado.
- CPU-56-04: leitura de PC: `pc+8` ARM; `pc+12` quando o operando é registrador com shift por registrador; STR/STM de PC armazena `pc+12`. Thumb: `pc+4`; LDR PC-relativo e ADD PC usam `(pc+4) & ~2`.
- CPU-56-05: loads MUST seguir o modelo ARM7TDMI: LDR desalinhado rotaciona a palavra lida do endereço alinhado; LDRH desalinhado rotaciona por 8; LDRSH desalinhado se comporta como LDRSB; STR/STRH alinham o endereço. Writeback e base na lista seguem o comportamento ARM7TDMI documentado (LDM com base na lista não faz writeback; STM com base na lista armazena o valor original se a base for o primeiro registrador da lista, senão o valor atualizado). Lista vazia transfere r15 e soma/subtrai 0x40.
- CPU-56-06: MUL/MLA/longos MUST calcular o resultado módulo 2^32/2^64 com N/Z atualizados; C e V preservados (o ARM7TDMI deixa C sem significado; preservar é escolha local documentada até fixture de hardware). Custo de ciclos segue o número de bytes significativos do multiplicador (m=1..4).
- CPU-56-07: exceções MUST salvar CPSR no SPSR do modo destino, gravar LR (SWI/UND: endereço da próxima instrução; IRQ: próxima instrução + 4; Reset sem LR), trocar modo, setar I (e F em Reset/FIQ), limpar T e saltar para o vetor. SWI MUST chamar o gancho de BIOS da SPEC-060 quando HLE estiver ativa; caso contrário entra no vetor 0x08 da BIOS fornecida.
- CPU-56-08: IRQ MUST ser avaliada somente na fronteira entre instruções, quando a linha IRQ estiver ativa e CPSR.I = 0.
- CPU-56-09: o custo de cada passo MUST ser computado pela soma dos acessos de bus que a instrução realiza (fetches sequenciais/não sequenciais e dados) mais ciclos internos (I) do ARM7TDMI: +1 para shift por registrador, +1 para loads, m (+1 para MLA, +1 para longos) para multiplicações, refill de pipeline (2 fetches) em branch/escrita de PC. O custo de cada acesso vem do bus (SPEC-057).
- CPU-56-10: o resultado de `step` MUST ser estruturado: ciclos consumidos, instrução aposentada ou exceção tomada, e fault (`unpredictable`, `bus_fault`) quando aplicável. Fault deixa o estado no ponto anterior à instrução.

## Interfaces, entradas e saídas

```rust
pub trait Bus {
    fn read8/16/32(&mut self, addr: u32, access: Access) -> u32;   // retorna valor e acumula ciclos
    fn write8/16/32(&mut self, addr: u32, value: u32, access: Access);
    fn idle(&mut self, cycles: u32);
    fn swi_hle(&mut self, cpu: &mut Cpu, comment: u8) -> Option<…>;  // SPEC-060
}
Cpu::step(&mut self, bus: &mut impl Bus) -> Step
```

`Access` distingue sequencial/não sequencial e código/dado. Endereços guest são `u32`; nenhum ponteiro host atravessa a interface.

## Erros, limites e observabilidade

Contadores por família de instruções aposentadas (IAC-10). Trace opcional por passo com PC, modo, encoding e registradores alterados. `unpredictable` inclui PC e encoding.

## Critérios de aceite e matriz de testes

| Caso | Verificação |
|---|---|
| 16 condições × 16 combinações NZCV | teste unitário exaustivo |
| Shifter LSL/LSR/ASR/ROR/RRX imediato e registrador com 0, 1, 31, 32, 33, 255, 256 | valores e carry |
| ADD/ADC/SUB/SBC/RSB/RSC limites signed/unsigned | N/Z/C/V |
| LDR/LDRH/LDRSH/LDRSB desalinhados | rotação/extensão |
| LDM/STM: 4 modos, writeback, base na lista, lista vazia, `^` | memória e registradores |
| MSR/MRS por campo, troca de modo e bancos | registradores bancados |
| SWI/UND/IRQ e retorno `MOVS pc, lr` / `SUBS pc, lr, #4` | CPSR/SPSR/LR |
| BX ARM↔Thumb, BL Thumb em duas metades | PC/modo |
| Thumb formatos 1–19 | unitários por formato |
| ROMs de teste de CPU independentes (ex.: jsmolka `arm.gba`/`thumb.gba`, MIT) quando disponíveis | passam; ausência registrada como skipped |
| Diferencial contra mGBA (SPEC-057) | mesmo estado de CPU por N instruções |

## Questões abertas

Valor exato de C após MUL no ARM7TDMI (documentado como "sem significado"); C é preservado até fixture de hardware.

## Referências e evidências

[SPEC-002](SPEC-002-armv4t-semantics.md), [SPEC-013](SPEC-013-banked-cpu-and-psr.md), [SPEC-055](SPEC-055-interpreter-assisted-compatibility.md), [ADR-041](../adr/ADR-041-interpreter-first-core.md). Fontes: ARM7TDMI Technical Reference Manual, ARM Architecture Reference Manual (ARMv4T), GBATEK (KNO-001).
