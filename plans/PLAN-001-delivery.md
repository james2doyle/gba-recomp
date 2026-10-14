# PLAN-001 — Incrementos verticais

O status está exclusivamente no [roadmap](../ROADMAP.md). Cada incremento deve ter contrato aceito antes da implementação correspondente.

## 1. Fundação e CPU mínima — GBA-005/010/020

GBA-005 fixa Rust/LLVM/bindings e os perfis dos dois hosts em ADR-004 e `toolchain.lock.toml`. GBA-010 tem scaffold, ABI v1, IR mínima e emissão LLVM; a fixture O0/O2 passou em macOS arm64, e o gate Linux x86-64 permanece pendente. GBA-020 especifica em SPEC-012 e implementa intake SHA-256, descoberta de branches diretos, decoder ARM restrito, dispatcher LLVM por PC e pacote nativo headless. A ROM Tiny Counter percorre seis instruções AOT e O0/O2 produzem o mesmo estado após 100 passos em macOS arm64. O programa só exercita registradores e flags; memória, MMIO e periféricos são os próximos contratos. A execução Mac não declara o host Linux suportado nem fecha GBA-010.

## 2. Hardware e dispatch — GBA-025/030/040/045/050

GBA-025 implementou ABI v2, estado bancado e `MSR CPSR` da SPEC-013, literal PC-ROM restrito da SPEC-014, `ADD Rd,PC,#imm` da SPEC-015, `STR` físico limitado da SPEC-016, `MOV Rd,Rm` com `LSL #0` da SPEC-017, `BX` ARM/Thumb da SPEC-018, `PUSH` Thumb da SPEC-019, MOV high-register Thumb da SPEC-020, MOVS imediato da SPEC-021, BL Thumb de duas metades da SPEC-022, HLE Thumb SWI #1 RegisterRamReset da SPEC-023, LSL Thumb imediato da SPEC-024, LDR literal Thumb de ROM da SPEC-025, ADD/SUB Thumb Format 2 da SPEC-026, STRH Thumb imediato da SPEC-027, STRB Thumb imediato da SPEC-028, LDRB Thumb imediato da SPEC-029 e ORRS Thumb Format 4 da SPEC-030. A SPEC-023 acrescenta ABI v4 e estado de máquina v1 só para os efeitos contratados; não fornece bus, PPU ou APU gerais. KNO-005 mantém decode, semântica runtime e lowering AOT em colunas separadas. As fixtures strict-AOT O0/O2 passaram para ADD/SUB, ORRS, propagação LDR→ADD→BX e ORR→BX, limites ROM, stores halfword/byte, loads byte, faults e rejeições sem pacote parcial; evidências e hashes estão em [STRB](../knowledge/evidence/gba020-thumb-strb-macos-arm64.json), [LDRB](../knowledge/evidence/gba020-thumb-ldrb-macos-arm64.json) e [ORRS](../knowledge/evidence/gba020-thumb-orr-register-macos-arm64.json). `cargo test --locked --workspace --jobs 10` passou com 41 testes em `gba-ir` e 44 em `gba-recomp`, e o gate completo passou em Darwin/arm64. A compilação estática de LeafGreen passou por ORRS em `0x0800099E` (`0x4318`) e parou em `0x080009A2`, raw `0x3201`, inferido como ADDS `r2,#1` Thumb Format 3. Não houve pacote LeafGreen, execução da ROM ou decompilação REA. As SPEC-027/028/029 limitam stores halfword/byte e load byte às EWRAM/IWRAM físicas, sem criar bus geral. Depois, ampliar as instruções alcançadas e completar checkpoints, IRQ e DMA antes de otimizações agressivas. Fixture futura escreve tiles/bitmap, responde a botões, produz tom com timer/FIFO e persiste backup; introduzir código ROM→RAM e overlay para testar invalidação. mGBA fixado por commit fornece referência externa; discrepâncias viram KNO. Gate: mesmo cenário AOT gera estado e saídas esperadas.

SPEC-030 implementa ORRS Thumb Format 4 strict-AOT, com atualização de N/Z e preservação de C/V. SPEC-031 implementa MOVS/CMP/ADDS/SUBS Thumb Format 3 no decoder, IR, descoberta e lowering strict-AOT. As fixtures O0/O2 passaram; consulte [KNO-004](../knowledge/KNO-004-rom-intake.md), [SRC-32](../knowledge/KNO-001-evidence.md) e a [evidência Format 3](../knowledge/evidence/gba020-thumb-format-3-macos-arm64.json). A fronteira estática atual é `0xDDF6` em `0x080009A6`, inferida como branch condicional `BLE` com alvo `0x08000996`; os valores de NZCV da ROM não foram observados. O próximo contrato deve descobrir os dois caminhos e usar NZCV em runtime.

A SPEC-031 agora está implementada e verificada em O0/O2. A descoberta estática mais recente de LeafGreen parou em `0xDDF6` (`BLE`, alvo inferido `0x08000996`) em `0x080009A6`; a próxima tranche precisa fechar destino e fallthrough estaticamente e avaliar NZCV apenas durante execução AOT. Consulte [evidência Format 3](../knowledge/evidence/gba020-thumb-format-3-macos-arm64.json).

## 3. Perfis e produto — GBA-060/070/080

Implementar parser v1 e validação de evidências, cache por digest, preferências separadas e menu aplicável em jogo. Validar perda de dispositivo, pause/resume, fullscreen rollback e herança global/jogo. Empacotar para Linux x86-64 e macOS arm64 e executar sem LLVM instalado. Gate: fluxo completo com controles, áudio, vídeo e save.

## 4. Primeiro título — GBA-090

Usar o hash de KNO-004, sem tomar nome do arquivo como identidade. O perfil existente é intake draft, não tuning confirmado. Descobrir entradas/overlays, SWIs e hardware efetivamente usado; registrar traces com REA quando o runtime existir. Fixar cenários de boot, navegação, entrada em gameplay e save/reload com input reproduzível. Medir misses e promover apenas os caminhos comprovados. Se código gerado exigir interpretação, declarar aot-compatible ou deixar strict-aot explicitamente bloqueado para aquele cenário.

## 5. Expansão — GBA-095/100

Repetir gates de emissão/link/runtime/frontend por destino; mobile e Web exigem seus adaptadores. Link/netplay e acessórios exigem especificações próprias, sem simular sucesso para hardware ausente.

## Riscos e mitigação

| Risco | Mitigação |
|---|---|
| LLVM otimiza semântica guest incorreta | IR de efeitos, verificador e diferencial O0/O2 |
| Timing aproximado causa regressão | Checkpoints e traces de bus/eventos antes de otimizar |
| Build de ROM cresce excessivamente | Shards, cache e budgets medidos |
| Cobertura incompleta parece sucesso | Rótulo por cenário e census de fallback |
| Fonte/oráculo repete mesmo bug | Fixture independente; hardware quando necessário |
| Licenças diferem entre módulos vendor | Inventário por arquivo antes de reutilização |

Fora da primeira entrega executável: compatibilidade universal, netplay, acessórios raros, ganhos de resolução interna do jogo e substituição de engines de áudio. Escopo pode crescer por novas specs, sem alterar retroativamente evidência existente.
