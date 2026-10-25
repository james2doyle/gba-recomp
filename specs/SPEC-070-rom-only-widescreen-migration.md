# SPEC-070 — Migração ROM-only para apresentação ampliada (perfil, patches e diagnóstico)

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).
Base: [SPEC-069](SPEC-069-widescreen-presentation.md) (superfície), [SPEC-008](SPEC-008-game-profiles.md) (perfil e PROF-01..08), [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md), [KNO-011](../knowledge/KNO-011-widescreen-survey.md).

## Problema e escopo

A SPEC-069 entrega a superfície ampliada, mas fixa que "jogo sem teto conhecido ⇒ 240". Na prática isso significa que nenhum título recebe margem, porque não existe parser de perfil: `crates/gba-desktop/src/settings.rs::view_ceiling` devolve `None` para toda ROM, inclusive para as que já têm perfil em `profiles/`.

Esta spec define o caminho **ROM-only**: o desenvolvedor não tem código-fonte e não vai recompilar o jogo. Tudo o que se pode fazer é

1. declarar, num perfil versionado e atrelado ao SHA-256, **qual largura** o título tolera;
2. alterar **poucos bytes** do ROM para corrigir **enquadramento** — limites de câmera, âncora de HUD, culling de sprites;
3. **medir** o resultado com um diagnóstico que diga se a margem é conteúdo real, enrolamento ou backdrop.

Escopo: contrato de dados e de diagnóstico. Não implementa nada por si; a sequência executável está em [PLAN-003](../plans/PLAN-003-widescreen-rom-migration.md) e o procedimento para o agente em [docs/widescreen-migration.md](../docs/widescreen-migration.md).

### Fora de escopo

- Recompilar o jogo ou editar código-fonte. É o caminho "source-available", que esta spec não cobre.
- Criar conteúdo de margem. Patches corrigem enquadramento; não aumentam o mapa (ver [Limite duro](#limite-duro)).
- Substituição de instrução no emissor LLVM (hook por PC). Registrada como adiada em [ADR-043](../adr/ADR-043-rom-patch-at-load.md).
- Distribuição de ROM alterada. O perfil contém offsets e bytes; a imagem final é montada em memória na máquina do usuário.
- Ampliação vertical (SPEC-069 continua limitando a 160 linhas).

## Limite duro

Esta seção existe para impedir a promessa errada, e é normativa.

- **Mapa de texto é 256 ou 512 texels.** Um BG de texto com `BGxCNT` size 0 expõe 256 colunas de mapa. Ampliar a superfície além de 256 mostra **enrolamento do anel** ou backdrop, nunca mundo novo. Nenhum patch de ROM contorna isso sem também realocar o mapa, o que é mudança de estrutura de dados do jogo e está fora do escopo desta spec.
- **Mapa affine vai até 1024 texels** (`128 << size`, size 3). Aqui a margem é conteúdo real e nenhum patch é necessário (WS-30 em [KNO-011](../knowledge/KNO-011-widescreen-survey.md)).
- **Bitmap (modos 3, 4, 5) é 240×160 por construção** (160×128 no modo 5). Não existe mapa maior; a margem é sempre backdrop (WS-69-09).
- **HUD e sprites em coordenadas fixas** aparecem deslocados ou fora da tela quando a largura muda, porque o jogo os posiciona para 240 colunas. Isso é corrigível por patch (âncora) ou inevitável (arte desenhada para 240).

Consequência de aceite: um perfil MUST NOT declarar `view_ceiling > 256` para um título cujo BG de texto visível use mapa 256 **e** cuja margem não seja intencionalmente backdrop/anel. O diagnóstico da seção abaixo é o que decide.

## Contrato de dados: seção `presentation` e `[[patch]]`

Extensão da v1 de [SPEC-008](SPEC-008-game-profiles.md), com a mesma rigidez: `version = 1` continua, chave desconhecida é erro, SHA-256 de 64 hex é obrigatório antes de qualquer efeito.

```toml
[presentation]
# Teto de largura que este título tolera. Entra em WS-69-19.
view_ceiling = 288

# Como preencher margem recusada: "backdrop" (palette[0], fiel) ou "black".
margin_fill = "backdrop"

# Política de OBJ na margem. Padrão false (recorte nativo, WS-69-10).
widen_obj = false

# Evidência que sustenta os números acima.
evidence = "KNO-011 WS-32, margem medida: ring 64 / authored 16 / backdrop 0"

[[patch]]
# Endereço na ROM (offset de arquivo), não endereço de execução.
offset = 0x1A2B4

# Bytes esperados na imagem original. Se não bater, o run aborta.
expected = "01 20 08 40"

# Substituição. Pode ser mais curta que `expected` (NOP-fill é explícito).
replacement = "00 20 08 40"

# Obrigatórios: motivo e evidência (PROF-03).
reason = "camera clamp derecho: 0x140 -> 0x180"
evidence = "KNO-012#camera-clamp"

# Escopo do efeito, para auditoria.
affects = "framing"   # framing | hud | culling
```

Regras:

- **PATCH-70-01:** todo `[[patch]]` MUST ser byte-verificado. `expected` que não coincide com a imagem carregada MUST abortar a execução com erro explícito, citando offset e os primeiros bytes divergentes. Não existe modo "aplicar mesmo assim".
- **PATCH-70-02:** patches MUST ser aplicados somente após o SHA-256 da imagem coincidir com `[image].sha256` (PROF-02). Título, game code, região e nome de arquivo não autorizam.
- **PATCH-70-03:** `expected` e `replacement` MUST ter o mesmo comprimento. Encurtar obriga a escrever o preenchimento (por exemplo `00` = `andeq r0,r0`) explicitamente, para que a intenção fique registrada.
- **PATCH-70-04:** offsets MUST ser disjuntos entre patches. Sobreposição é erro de perfil, não "último ganha".
- **PATCH-70-05:** nenhum patch MUST conter código, script ou expressão. São bytes literais; qualquer coisa executável é rejeitada pelo parser (PROF-03).
- **PATCH-70-06:** todo patch MUST declarar `reason`, `evidence` e `affects`. O log de execução MUST imprimir os três, mais o offset, no momento da aplicação.
- **PATCH-70-07:** `view_ceiling` MUST estar em `240..=384` (WS-69-05) e MUST NOT exceder o que o diagnóstico sustenta (ver Critérios de aceite).
- **PATCH-70-08:** `presentation` ausente ⇒ 240, sem erro, sem aviso em nível de erro (WS-69-19). Perfil presente mas sem `presentation` é válido.

Endereçamento: **offset de arquivo na ROM**, nunca endereço de execução. A conversão para endereço de jogo é `endereço = offset + 0x0800_0000` para a região de cartucho inicial; overlays e bancos mapeados são declarados com `[overlays]` em SPEC-008 e ficam fora desta v1.

## Requisitos de diagnóstico

O diagnóstico é a única forma de um agente saber se a migração funcionou, e é ele que impede a promessa falsa do [Limite duro](#limite-duro).

- **DIAG-70-01:** o host MUST oferecer um relatório de margem (`--margin-report`) que, para o frame indicado, classifique cada coluna de margem como `ring`, `backdrop` ou `authored`.
- **DIAG-70-02:** a classificação MUST ser estrutural, não heurística de cor:
  - `ring`: a coluna de saída `out` em margem amostra uma posição que a própria linha nativa também amostra; para mapa de texto de 256, isso equivale a `coluna(256 + k) == coluna(k)` à direita e `coluna(-k) == coluna(256 - k)` à esquerda, com a ressalva de que a cauda do mapa (`240..255`) não é alcançável pela linha nativa; mapa affine com wrap repete a cada período do passeio e é classificado pela mesma igualdade de posição amostrada;
  - `backdrop`: a coluna resolve para `palette[0]` e nenhuma camada a cobriu;
  - `authored`: qualquer outra coisa (posição de mapa alcançável só pela margem — a cauda de um mapa de 256, conteúdo de mapa affine sem wrap maior que a janela, mapa 512, ou patch de HUD).
- **DIAG-70-03:** o relatório MUST listar modo de vídeo, por BG: tipo (texto/affine/bitmap), `BGxCNT` decodificado, tamanho do mapa e enable; e para affine, `PA/PB/PC/PD` e o ponto de referência travado da linha.
- **DIAG-70-04:** o relatório MUST declarar o veredito de largura: para cada `view_ceiling` candidato, se a margem é majoritariamente `authored`, `ring` ou `backdrop`, com contagem por linha.
- **DIAG-70-05:** o relatório MUST NOT alterar o estado guest. É leitura: não aplica patch, não muda `view`, não conta como frame.

## Precedência e interação com SPEC-069

`PROF-06` e `PROF-07` continuam valendo. Para largura:

```
efetiva = override_sessão  (--view-width, WS-69-18, nunca persistido)
        | min(preferência do usuário, view_ceiling do perfil)
        | 240                                        (sem perfil)
```

A preferência é um **teto do usuário**: com a preferência no padrão 240, um perfil com `view_ceiling = 288` **não** amplia nada; o jogador amplia subindo a preferência, e o teto do perfil volta a limitar por cima. Nada disso vale para executores headless (`gba-run`, `gba-native-rt` sem janela), que não têm fonte de preferência: para eles `efetiva = override_sessão || view_ceiling || 240`, e o teto do perfil é a largura que a verificação exercita. Essa é a única instância em que o teto, sozinho, amplia a superfície.

- **PATCH-70-09:** `--view-width` MUST continuar vencendo o teto do perfil (WS-69-18), e a UI MUST rotular o override como experimental.
- **PATCH-70-10:** o teto MUST ser lido do perfil do SHA exato. Perfil de outra revisão MUST ser ignorado com diagnóstico, não aplicado "por proximidade".
- **PATCH-70-11:** patches e `view_ceiling` MUST fazer parte do digest de código do pacote (PROF-05); mudar qualquer um deles invalida artefatos dependentes.

## Interfaces

Pretendido; nomes podem mudar, semântica é normativa.

```rust
/// Perfil v1 resolvido para uma imagem concreta.
pub struct Presentation {
    pub view_ceiling: Option<u16>,      // 240..=384
    pub margin_fill: MarginFill,        // Backdrop | Black
    pub widen_obj: bool,
    pub evidence: String,
}

pub struct RomPatch {
    pub offset: usize,
    pub expected: Vec<u8>,
    pub replacement: Vec<u8>,
    pub reason: String,
    pub evidence: String,
    pub affects: PatchScope,            // Framing | Hud | Culling
}

/// Aplica patches verificados à imagem em memória. Falha alto e cedo.
pub fn apply_patches(rom: &mut [u8], patches: &[RomPatch]) -> Result<Vec<AppliedPatch>, PatchError>;
```

`apply_patches` recebe a imagem já verificada por SHA; a verificação de hash é responsabilidade do chamador e MUST acontecer antes.

## Critérios de aceite

| ID | Caso | Esperado |
|---|---|---|
| A-70-01 | Perfil ausente | 240, sem erro, sem patch |
| A-70-02 | Perfil com `view_ceiling = 288` e preferência 320 | Efetivo 288; UI mostra 288 |
| A-70-03 | `--view-width 320` com teto 288 | 320, rotulado experimental |
| A-70-04 | SHA da imagem difere do perfil | Perfil ignorado; diagnóstico; execução continua em 240 |
| A-70-05 | `expected` não confere | Aborta com offset e bytes divergentes; nada é aplicado |
| A-70-06 | Offsets sobrepostos | Perfil rejeitado na carga |
| A-70-07 | `expected` e `replacement` de tamanhos diferentes | Perfil rejeitado |
| A-70-08 | Patch de câmera num título de mapa 256 e teto 288 | Aceito **somente** se o diagnóstico classificar a margem como `authored`; se `ring`/`backdrop`, rejeitar o teto e registrar por quê |
| A-70-09 | `--margin-report` sobre o Rkanoid a 320 px | `BG0` texto com `map_width=256`; esquerda `ring=40`, direita `ring=24`/`authored=16`, `backdrop=0`; `hw 256+k` igual a `hw k` em todas as linhas (WS-32) |
| A-70-10 | Hash de frame com e sem patches | Difere (patches mudam o guest); sem patches, idêntico ao baseline |
| A-70-11 | Perfil com chave desconhecida | Rejeitado (PROF-01) |
| A-70-12 | Perfil com `view_ceiling = 288` e preferência padrão (240) | Efetivo 240: a preferência é um teto do usuário e é preservada intacta para quando houver preferência maior |

Critério de conclusão: A-70-01..12 como testes verdes, mais uma execução real de ponta a ponta registrada em `knowledge/evidence/` com ROM, SHA, perfil e relatório de margem.

## Riscos

| Risco | Mitigação |
|---|---|
| Patch "funciona" e quebra outro caminho do jogo | `expected` byte-exato, escopo declarado, hash de frame registrado antes/depois, e cenário de teste nomeado |
| Prometer mundo novo onde só há anel | `Limite duro` normativo + A-70-08 condicionado ao diagnóstico |
| Perfil de outra revisão aplicado | PATCH-70-02/10 e A-70-04 |
| Patches acumularem dívida invisível | Todo patch no perfil, com evidência; nada embutido em código |
| Diagnóstico virar oráculo de cor | DIAG-70-02 exige classificação estrutural |

## Questões abertas

- Qual título é o primeiro alvo com SHA fixado? Sem ele, GBA-110 continua sem critério de aceite.
- Patches em código Thumb imediato versus ARM literal: os exemplos da v1 são bytes puros, mas a ergonomia de escrever offsets à mão é ruim. Vale um `[[patch]]` com `instructions = ["mov r0,#0x140"]` **traduzido no build** para bytes verificados? Isso melhora a revisão e mantém bytes no artefato.
- Como expressar patch que depende de estado (por exemplo, só quando `DISPCNT.mode == 0`)? A v1 não tem condição; um título que troque de modo pode precisar.
- Se o jogo copia seu laço de render para IWRAM, o patch de ROM não alcança a cópia. Detectável só em runtime; registrar como limitação conhecida.
- `margin_fill = "black"` ainda não tem preferência de usuário (SPEC-069 deixa em aberto); esta spec só permite declarar por perfil.

## Referências

[SPEC-069](SPEC-069-widescreen-presentation.md), [SPEC-008](SPEC-008-game-profiles.md), [ADR-042](../adr/ADR-042-widescreen-presentation-surface.md), [ADR-043](../adr/ADR-043-rom-patch-at-load.md), [KNO-011](../knowledge/KNO-011-widescreen-survey.md) (WS-16–WS-20, WS-30/31 medidos nesta máquina), [plans/PLAN-003](../plans/PLAN-003-widescreen-rom-migration.md), [docs/widescreen-migration.md](../docs/widescreen-migration.md).
