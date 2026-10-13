# SPEC-008 — Perfis de ajuste fino por jogo

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Representar conhecimento sobre uma revisão exata sem misturar preferências de usuário, heurística de descoberta e garantia de compatibilidade.

## Contrato de dados v1

Formato TOML UTF-8, `format="gba-recomp-profile"`, `version=1`, `id` estável, `status="draft"|"validated"`, `[image]` com `sha256` hexadecimal de 64 caracteres, tamanho, game code/revisão descritivos. `status=validated` exige relatório que identifica cenários e versões; o rótulo nunca substitui validação de hash.

| Seção | Conteúdo e regra |
|---|---|
| `analysis` | Seeds ARM/Thumb, code/data ranges, destinos indiretos, origem/evidência |
| `overlays` | Origem local/hash, endereço de execução, extensão, entradas, transformação versionada |
| `runtime` | BIOS, backup, RTC/periféricos, política de timing e fallback |
| `compiler` | O0/O2, sharding, otimizações permitidas por bloco, budget de análise |
| `presentation_defaults` | Sugestões vídeo/áudio/controles, sem substituir preferência explícita |
| `evidence` | IDs KNO/REA, hash de trace/cenário, cobertura e questões abertas |

Valores `unresolved` são admitidos em perfil draft para BIOS/backup/RTC e MUST impedir pacote qualificado. `analysis.max_blocks` limita descoberta; `analysis.seeds` contém address/mode/origin/evidence. Os campos e tabelas dos exemplos versionados em `profiles/` integram o vocabulário v1; extensão exige mudança deste contrato e validação.

## Requisitos normativos

- PROF-01: schema MUST rejeitar versão/chave desconhecida, tipo incorreto, hash inválido, range fora da imagem, sobreposição contraditória e entrada mal alinhada. Ranges usam fim exclusivo; ARM múltiplo de 4, Thumb múltiplo de 2, modo em campo separado do endereço.
- PROF-02: perfil MUST casar SHA-256 exato antes de aplicar hints/overrides. Revisões, traduções e patches têm perfis distintos; título/game code não autorizam herança sem validação.
- PROF-03: regras técnicas MUST informar motivo, evidência e escopo. Seeds são candidatos; bounds de tabelas e patches/hooks exigem precondições. Hook nativo só pode referenciar implementação compilada e registrada, com hash original, ABI e efeitos declarados; scripts/comandos em TOML são proibidos.
- PROF-04: `strict-aot` MUST rejeitar qualquer solicitação de fallback interpretado. Perfil draft MAY ser usado para análise, mas build de pacote validado exige capacidades resolvidas e relatório correspondente. Um override que pula timing exige modo experimental anunciado e não recebe selo de conformidade.
- PROF-05: digest compilado MUST incluir hints, overlays, hooks e opções que afetam código. Digest semântico MUST incluir BIOS, hardware, timing e RTC; preferências host ficam fora dele. Mudança de dado relevante invalida artefatos dependentes.
- PROF-06: precedência técnica MUST ser baseline → perfil exato → override local explícito validado → CLI. Contratos de segurança semântica (hash, ABI, guards necessários) não são desligáveis pela precedência. Overrides são materializados no manifesto.
- PROF-07: preferências MUST resolver em defaults do app → sugestões do perfil → global usuário → override usuário/jogo → sessão/CLI. UI mostra valor efetivo e origem. Remover override de jogo retorna à herança.
- PROF-08: parser MUST impor limites de tamanho (baseline 1 MiB TOML), entradas (100.000), profundidade de dependências e paths locais; coleções maiores precisam sidecar versionado com hash. Paths de snapshot não podem escapar da raiz escolhida para o projeto.

## Interfaces, erros e aceite

`profile validate`, `profile explain`, `profile import-hints`, `profile diff`. Saída determinística com regras aplicadas, ignoradas e conflitantes. Erros `profile_hash_mismatch`, `profile_schema_invalid`, `profile_conflict`, `profile_unproven_override`.

Matriz: mesmo nome/hash diferente; duas revisões; typo em chave; destino fora da ROM; ARM/Thumb desalinhado; overlay conflitante; perfil malicioso com comando; sugestão sobreposta por global/jogo/CLI; cache após opção compilada versus volume. Ver exemplos em profiles/.

## Questões e fontes

Schemas mecânicos serão implementados junto ao parser; a norma v1 é este documento e exemplos. Catálogo de hooks só cresce com spec própria. KNO-001 SRC-02/03/08 e ADR-002.
