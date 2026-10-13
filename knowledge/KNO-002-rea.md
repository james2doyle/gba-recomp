# KNO-002 — Uso do REA e limite da evidência

Data: 2026-10-13. Skill aplicada: `/Users/celsoinacio/.agents/skills/reverse-engineer-anything/SKILL.md` (metadata v37).

A skill orienta ferramentas comuns para fontes completas e REA para artefatos/runtime. O inventário local de `vendor-ref/gba-recomp` não encontrou ROM `.gba`, ELF ou biblioteca compilada para analisar. Naquele momento ainda não havia jogo alvo. Depois, o usuário adicionou uma ROM: a tentativa real de `open_binary` e o erro de formato estão em [KNO-004](KNO-004-rom-intake.md). Não houve execução de jogo.

O MCP REA foi usado efetivamente com `record_unknown` para preservar a lacuna de cobertura AOT e com `export_evidence_bundle` para exportá-la em [evidence/rea-session.json](evidence/rea-session.json).

- Unknown: `unk_57496dfa528f40de8e35fd1e6f9bd2f9edea204d433f113011b6846934899bc9`.
- Evidence da mutação do registro: `ev_c81716b97a0f03b0de49d3095a2b0e0175e36db1d2af888fae84aa7d723babf2`.
- Resultado: questão aberta, severidade alta, sem observações de suporte ou contradição.
- Exportação inicial: 1 registro e 1 unknown; o bundle foi atualizado com a segunda questão vinculada à ROM. Esse Evidence ID comprova o registro da pergunta; **não comprova comportamento de hardware ou de jogo**.

## Investigação prevista por jogo

1. Vincular ROM e perfil ao SHA-256; identificar símbolos/ELF e origem dos bytes. Não presumir que `open_binary` suporta `.gba` raw, loader GBA ou estado Thumb configurável.
2. Para ELF compatível, abrir alvo, recuperar entradas, CFG e chamadas indiretas com operações anunciadas pelo servidor. Preservar bytes/endereço/modo e incertezas. Não converter pseudocódigo em semântica autoritativa.
3. Se o provider não suporta ROM raw, usar o decoder próprio para a imagem e REA apenas nos artefatos que suporta. Uma imagem derivada deve manter mapa de offsets e nunca se passar pelo original.
4. Com runtime disponível, capturar cenário determinístico autorizado via `capture_process_scenario`: comando, hash, input, frames, timeout, versão, exit code e traces. Não extrapolar resultado além da janela.
5. Correlacionar misses, writes em código, MMIO, IRQ e digest de frames/áudio; propor hints. Importar dados via validação do perfil, sem executar comandos oriundos de análise.
6. Exportar bundle por investigação, registrar IDs em KNO, encerrar sessão nativa quando aberta e resolver unknown somente com evidência adequada.

A especificação cobre o processo. Cobertura AOT por título permanece explicitamente não resolvida.

## Continuação em 2026-10-13

Na retomada para implementar GBA-020, a chamada `mcp__rea__binary_session({})` falhou no transporte com `Transport closed`. `command -v rea` não encontrou CLI instalada nesta shell. Não foi repetida a abertura da ROM raw, pois KNO-004 já registra `unsupported binary format`; não há Evidence ID novo nem observação de análise. A investigação do código fonte continua por leitura local. A captura REA por ROM segue pendente até reconexão ou suporte de formato.

Na continuação da implementação SPEC-025, uma nova chamada `mcp__rea__binary_session({})` também retornou `Transport closed`; a CLI `rea` continuou ausente. Não foi repetida a mesma abertura da ROM raw recusada pelo provider. Esta tentativa não gerou Evidence ID, decompilação ou observação sobre o conteúdo da ROM; as referências de decoder foram consultadas diretamente nos repositórios locais fixados.
