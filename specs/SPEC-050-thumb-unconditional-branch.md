# SPEC-050 — Branch incondicional Thumb Format 18 em AOT estrito

Estado do documento: Accepted. Especificação escrita antes da implementação; critérios exercitados em [evidência O0/O2](../knowledge/evidence/gba020-thumb-unconditional-branch-macos-arm64.json). Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Após SPEC-049, a compilação estática O2 da ROM LeafGreen fixada em [KNO-004](../knowledge/KNO-004-rom-intake.md) avançou até `0x08000AE6`, halfword observado `0xE00A`, e parou com `unsupported_thumb_instruction`. A leitura do encoding como Thumb `B` Format 18 e o alvo `0x08000AFE` são inferências. Nenhum PC ou estado guest foi observado em execução. A evidência fixa o compiler, a ROM, o comando, a saída e os hashes em [leafgreen-after-dma3-20261014-O2.json](../knowledge/evidence/leafgreen-after-dma3-20261014-O2.json).

Esta tranche acrescenta somente branch incondicional Thumb Format 18 ao decoder, GBA IR, discovery e lowering LLVM strict-AOT. Não inclui BL, branches condicionais, BX, branches ARM, nem qualquer inferência de flags.

## Requisitos normativos

- TUB-01: o decoder MUST reconhecer apenas `(encoding & 0xF800) == 0xE000`. Deve interpretar `imm11 = encoding & 0x07FF` como inteiro signed de 11 bits, deslocar o valor assinado uma posição à esquerda e calcular `target = guest_pc.wrapping_add(4).wrapping_add(displacement)`. `guest_pc` MUST estar alinhado a halfword.
- TUB-02: `ThumbOperation::UnconditionalBranch { target }` MUST representar a instrução sem máscara de sucessores nem condição. O verificador MUST conferir operação, encoding e alvo calculado; origem e alvo devem ser alinhados a halfword, e a instrução Thumb no alvo deve existir no programa verificado. IR inconsistente ou alvo ausente MUST ser rejeitado.
- TUB-03: discovery MUST enfileirar somente o alvo com o mesmo estado abstrato de registradores, flags e memória. Não deve enfileirar `guest_pc+2` como consequência desse branch. Alvo fora da imagem ROM, desalinhado ou sem instrução decodificável MUST falhar antes de publicar pacote parcial.
- TUB-04: na execução, a operação MUST gravar `PC=target`; `r0–r14`, CPSR, modo, memória e estado de hardware MUST permanecer inalterados. O ciclo abstrato MUST avançar uma unidade.
- TUB-05: LLVM MUST emitir uma saída direta ao dispatcher Thumb existente com o alvo constante. A execução deve permanecer strict-AOT, sem interpretação, JIT ou fallback.
- TUB-06: `GBA_IR_VERSION`, ABI/runtime, estado de máquina e semântica existente MUST permanecer inalterados.
- TUB-07: instruções alcançáveis não suportadas no destino MUST falhar durante compilação e não podem deixar pacote parcial.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| `0xE000`, `0xE001`, `0xE7FF` | Decoder calcula deslocamentos zero, positivo e negativo com sinal de 11 bits |
| `0xE00A` em `0x08000AE6` | Alvo decodificado em `0x08000AFE` |
| Cálculo próximo aos limites de `u32` | Wrap modular e alinhamento halfword preservados |
| Encoding diferente do Format 18 | Continua rejeitado pelo subconjunto atual |
| IR adulterada ou destino ausente | Verificador rejeita encoding/alvo inconsistente ou sucessor sem instrução |
| Discovery de branch | Somente o alvo é enfileirado; o fallthrough não é criado por esta instrução |
| Execução da IR | PC aponta ao alvo, ciclo avança em um e os outros campos ficam preservados |
| Fixtures O0/O2 | Executável percorre o branch direto; manifesto declara strict-AOT e não há fallback |
| Alvo não compilável ou fora da ROM | Erro aponta a instrução/destino e não publica pacote parcial |
| LeafGreen fixada | A tentativa passa por `0xE00A` e registra a próxima fronteira estática em `0x08000BE4`; não afirma execução do jogo |

## Fixtures e verificações

Adicionar testes de decoder para imediatos positivo, zero, negativo e wrap; testes do verificador para encoding, alinhamento e alvo ausente; discovery para inclusão exclusiva do destino; semântica da IR para preservação do estado e ciclo; e fixtures strict-AOT O0/O2 com branch tomado, caminho posterior inválido e rejeição sem pacote parcial. Executar a suíte workspace e o gate GBA-020 dentro do orçamento de 10 CPUs detectado. Repetir a descoberta O2 da LeafGreen com o SHA-256 de KNO-004 e registrar o resultado em JSON e log com hashes.

Fixtures sintéticas validam somente os contratos e estados iniciais nelas configurados. Avançar na discovery da ROM não prova execução dinâmica, compatibilidade ou suporte ao título.

## Evidência de implementação e aceite

O decoder, a GBA IR, a discovery e o lowering LLVM foram implementados no commit `0ebcefa47120c6288fd6a0948ab746699b14dd5e`; `GBA_IR_VERSION` permaneceu em 2 e nenhuma ABI foi alterada. A suíte `cargo test --locked --workspace --jobs 10` passou com 66 testes em `gba-ir` e 113 em `gba-recomp`. O gate GBA-020 completo passou em macOS arm64 com dez CPUs lógicas: a fixture strict-AOT O0/O2 executou o branch, produziu o mesmo estado final e os casos de alvo não suportado/fora da ROM foram rejeitados sem pacote parcial. Os logs e hashes estão em [evidência do gate](../knowledge/evidence/gba020-thumb-unconditional-branch-macos-arm64.json).

Na ROM LeafGreen fixada, o compile O2 passou estaticamente do halfword `0xE00A` em `0x08000AE6` e parou em `0x4002` em `0x08000BE4`, ainda antes de LLVM/link. O resultado não cria pacote e não demonstra execução do título; veja [registro da tentativa](../knowledge/evidence/leafgreen-after-thumb-b-20261014-O2.json).

## Relação com specs e referências

- [SPEC-003 — discovery e CFG](SPEC-003-discovery-overlays.md), CFG-01/06/08.
- [SPEC-004 — GBA IR e LLVM](SPEC-004-ir-llvm.md).
- [SPEC-018 — BX ARM/Thumb](SPEC-018-arm-thumb-branch-exchange.md); sem mudança no contrato BX.
- [SPEC-032 — branch condicional Thumb](SPEC-032-thumb-conditional-branch.md); este contrato adiciona somente Format 18.
- [ADR-035 — branch Thumb direto](../adr/ADR-035-thumb-unconditional-branch.md).
- [SRC-44 — encoding e semântica da referência](../knowledge/KNO-001-evidence.md).
- [KNO-004 — identidade e tentativas LeafGreen](../knowledge/KNO-004-rom-intake.md).
