# ADR-031 — Latches IE/IF/IME sem entrega de IRQ

- Estado: Accepted
- Data: 2026-10-14
- Implementação: `4003a1ae9206b7acdb8d78fd97fd26d3e931d681`
- Supersede: —

## Contexto

A análise estática após SPEC-044 alcança uma sequência com LDRH em IME, STRH para IE e restore de IME. mGBA confirma a semântica dos latches, mas também agenda IRQ quando `IE & IF != 0`. O runtime local ainda não implementa fontes de eventos nem despacho de IRQ. Já o runner strict-AOT inicia a máquina zerada e não existe produtor local de IF; nesse perfil IF permanece zero.

## Decisão

Implementar [SPEC-045](../specs/SPEC-045-thumb-ie-ime-latches.md) como subconjunto fechado: loads halfword dos latches IE/IF/IME; stores para IE/IME somente com IF zero; IF writes e outros efeitos I/O faultam. Os helpers novos são aditivos a ABI v4. A discovery começa com latches zero e atualiza fatos abstratos. O manifesto declara `interrupt_model: none` e `io_initialization: all-zero`.

No lowering Thumb STRH, SPEC-045/IOM-05 substitui somente a seleção de helper exigida por IME-05 da SPEC-044: usa `gba_store16_v5`. O helper legado `gba_store16_v4` mantém seu contrato para consumidores diretos.

O runtime não simula uma IRQ. Como nenhuma fonte pode elevar IF no perfil atual, IE/IME não podem criar IRQ pendente. O runner gerado zera a máquina inteira antes do primeiro bloco compilado, satisfazendo a precondição declarada no manifesto; a ABI do bloco não valida latches iniciais. Um host externo que invoque blocos diretamente com latches não zero está fora do contrato atual. O helper de store ainda verifica IF zero em tempo de execução e retorna fault code 8 quando a precondição falha. Uma futura fonte de eventos exige nova especificação de agendamento/IRQ e revisão das regras de preservação da sombra.

## Alternativas

- Presumir IE/IME como stores inertes em qualquer estado: rejeitada, pois com IF pendente as escritas reavaliam IRQ.
- Implementar temporização, fontes de eventos, entrada e retorno de IRQ nesta tranche: adiada para evitar misturar semântica de barramento do slice com um controlador ainda inexistente.
- Rejeitar todos os stores IE/IME: desnecessário no perfil fechado do runner, que começa com IF zero e não tem produtores de eventos.
- Alterar os helpers congelados `gba_load16_v3`/`gba_store16_v4`: rejeitada; os novos efeitos usam símbolos aditivos.

## Consequências e limites

O fluxo de seção crítica da LeafGreen pode avançar na compilação sem fingir suporte geral a IRQ. O estado zero decorre do runner local, não de uma inicialização completa do GBA físico. Sem PPU/timers e outros produtores de IF, o resultado não representa o comportamento temporal de hardware. A compilação ou fixture aprovada não é evidência de execução da ROM comercial.

## Referências

- [SPEC-045](../specs/SPEC-045-thumb-ie-ime-latches.md)
- [SPEC-044](../specs/SPEC-044-thumb-ime-store.md)
- [ADR-030](ADR-030-thumb-ime-store.md)
- [SRC-41](../knowledge/KNO-001-evidence.md)
- [ROADMAP](../ROADMAP.md)

## Validação

`cargo test --locked --workspace --jobs 10` passou com 64 testes em gba-ir e 91 em gba-recomp. O gate GBA-020 passou em macOS arm64, com fixtures O0/O2 e harness C; a tentativa LeafGreen ainda não produz pacote. Os hashes e limites estão em [gba020-thumb-ie-ime-latches-macos-arm64.json](../knowledge/evidence/gba020-thumb-ie-ime-latches-macos-arm64.json).
