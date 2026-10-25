# Migrar um jogo ROM-only para apresentação ampliada

Procedimento para um agente que recebe **uma ROM e nada mais** e quer que ela use a superfície ampliada. Contrato: [SPEC-070](../specs/SPEC-070-rom-only-widescreen-migration.md). Decisão de mecanismo: [ADR-043](../adr/ADR-043-rom-patch-at-load.md). Sequência de implementação: [PLAN-003](../plans/PLAN-003-widescreen-rom-migration.md).

Leia a regra antes de começar, porque ela decide a maioria dos casos:

> **Ampliar a superfície não cria mundo.** O PPU só tem o que o jogo escreveu no mapa. Um BG de texto tem 256 ou 512 texels de largura; um affine tem até 1024; bitmap é 240×160. Se a margem não é conteúdo de mapa, ela é anel (repetição) ou backdrop. Nenhum patch de ROM muda isso sem reescrever a estrutura de dados do jogo.

Portanto este guia é, na maior parte, um procedimento para **decidir e documentar um "não"** — e só em alguns casos para aplicar patches de enquadramento.

## 0. Preparação

```bash
cargo build --release -p gba-run
export ROM=games/titulo.gba
sha256sum "$ROM"   # este hash identifica a revisão e vai no perfil
```

Fixe o hash desde o início. Revisões, traduções e patches têm perfis distintos; nada herda por proximidade (PROF-02, PATCH-70-10).

## 1. Inventário: o que o jogo realmente desenha

Rode com uma largura ampliada e peça o relatório de margem:

```bash
./target/release/gba-run --rom "$ROM" --frames 400 --input 60:200:RIGHT \
    --view-width 320 --margin-report
```

A saída lista modo de vídeo, cada BG (tipo, `BGxCNT`, largura de mapa) e a classificação da margem:

```
display: mode=0 dispcnt=0x1140
BG0: enabled=true kind=text cnt=0x00C7 map_width=256 char_block=1 screen_block=0
BG1: enabled=false kind=text cnt=0x0285 map_width=- char_block=1 screen_block=2
left margin:  backdrop=0 ring=40 authored=0 verdict=ring
right margin: backdrop=0 ring=24 authored=16 verdict=ring
```

Registre o resultado **em mais de um ponto do jogo**: título, gameplay, menu, pausa, game over. O modo de vídeo e o mapa mudam entre telas, e um teto declarado com base numa só tela produz defeito nas outras.

## 2. Portão de viabilidade: decida antes de patchar

| Situação medida | Veredito | Ação |
|---|---|---|
| Margem majoritariamente `authored` com BG affine (mapa até 1024) | **Vale ampliar** | Declare o teto; provavelmente nenhum patch é necessário |
| Margem `authored` com BG de texto de mapa **512** | **Vale ampliar** | Declare o teto; verifique HUD |
| Margem majoritariamente `ring` (mapa 256) | **Não vale** | Teto 240. Nenhum patch de câmera resolve |
| Margem majoritariamente `backdrop` | **Não vale** | Teto 240, salvo se o jogo usar a margem para HUD |
| Bitmap (modos 3/4/5) | **Nunca** | 240×160 por construção |
| HUD/placar desenhado em coordenadas fixas de 240 | Depende | Patch de âncora (`affects = "hud"`) **ou** recusar |

Exemplo real (Rkanoid v3.00, SHA `99ae82…3d94`): BG0 é texto com mapa 256; margem esquerda 40 `ring`, direita 24 `ring` + 16 `authored` (as últimas 16 colunas do mesmo mapa). **Teto correto: 240.** Declarar 288 prometeria margem que não existe. Ver WS-32 em [KNO-011](../knowledge/KNO-011-widescreen-survey.md).

Se o portão der "não vale", pule para a seção 6 e registre a recusa com a medição. **Isso é um resultado válido e útil**, não um fracasso.

## 3. Localizar o que patchar (só se o portão aprovou)

Você precisa do ponto exato onde o guest decide algo de enquadramento:

- **Limite de câmera**: uma comparação do tipo `cmp r0, #0x140` seguida de clamp, ou um `min` sobre a posição da câmera. É o caso mais comum e o mais fácil.
- **Âncora de HUD**: o deslocamento de OAM ou o scroll do BG de HUD calculado a partir de 240.
- **Culling de sprite**: comparação `x >= 240` que **para de desenhar** o objeto. Cuidado: no core, `x >= 240` em OAM é decodificação (`x - 256`), não clipping; não confunda os dois ao ler o desassemblado.

Ferramentas: desassemblador de sua preferência sobre a ROM, mais os offsets de arquivo que você confirmar por bytes. Anote o offset de **arquivo**, não o endereço de execução; a conversão é `offset + 0x0800_0000` na região inicial de cartucho.

## 4. Escrever o perfil

```toml
format = "gba-recomp-profile"
version = 1
id = "titulo-rev1"
status = "draft"

[image]
sha256 = "<64 hex do sha256sum>"
size = <tamanho em bytes>

[presentation]
view_ceiling = 288
margin_fill = "backdrop"
widen_obj = false
evidence = "KNO-012; margem medida: authored 80/80"

[[patch]]
offset = 0x1A2B4
expected = "40 01 00 00"     # bytes da imagem ORIGINAL
replacement = "80 01 00 00"  # mesmo comprimento
reason = "limite de camera 0x140 -> 0x180"
evidence = "KNO-012#camera-clamp"
affects = "framing"
```

Regras que o parser impõe (e que você não deve contornar):

- `expected` e `replacement` têm o **mesmo comprimento**; para encurtar, escreva o preenchimento explícito.
- Offsets **disjuntos** entre patches.
- `reason`, `evidence` e `affects` obrigatórios.
- Nada de mnemônico, expressão ou script: apenas bytes hexadecimais.
- `view_ceiling` em `240..=384`.

## 5. Verificar

```bash
export GBA_PROFILE_DIR=/caminho/do/perfil   # ou instale em ~/.config/gba-recomp/profiles
./target/release/gba-run --rom "$ROM" --frames 400 --margin-report
```

Confira, nesta ordem:

1. **O patch foi aplicado e logado** — o log mostra offset, escopo, motivo e evidência. Se `expected` não bater, o run aborta com exit 1 e a imagem fica intacta.
2. **A largura efetiva é a esperada** — `view_width` no JSON de saída.
3. **A margem é o que você prometeu** — `--margin-report` deve dar `authored` majoritário. Se der `ring`/`backdrop`, o teto está errado (A-70-08).
4. **O recorte canônico não mudou** — `frame_hash` com o perfil **sem patches** deve ser idêntico ao baseline. Com patches, ele **muda**: isso é esperado e é a prova de que o patch chega ao guest, não de que quebrou.
5. **Nada regrediu** — rode também em 240 e compare com o baseline sem perfil.

```bash
# baseline
./target/release/gba-run --rom "$ROM" --frames 400 | tail -1
# com perfil
GBA_PROFILE_DIR=... ./target/release/gba-run --rom "$ROM" --frames 400 | tail -1
```

## 6. Registrar

- Perfil em `profiles/` (draft até haver relatório de cenários).
- Medição em `knowledge/evidence/`: SHA, frame, largura, contagem por região, veredito.
- Achado em `knowledge/KNO-*.md` com WS-xx.
- Se recusou: registre **por quê**, com os números. Um "não" medido vale mais que um "sim" inventado.

## Armadilhas já pagas neste projeto

Estas custaram horas e vão custar de novo a quem não ler:

1. **`BG2PC` não é `BG2PD`.** `PC` é o cisalhamento vertical por pixel horizontal; definir `PC` em vez de `PD` faz o mapa sair na diagonal e parece bug do renderer. Em fixture de teste, `PA=PD=0x100`, `PB=PC=0`.
2. **Screen block e char block se sobrepõem.** Mapa em screen block 16 com char block 0 escreve por cima dos tiles; os primeiros 24 pixels ficam certos e o resto "some". Sintoma clássico de fixture errada.
3. **`x >= 240` em OAM é decodificação, não clipping.** Sprites de 256..511 são válidos (x negativo). Não trate como culling ao ler código.
4. **Medição circular.** Comparar a margem com a borda do próprio recorte sempre "prova" o que se quer. A verificação correta compara `hw` com `hw` dentro da superfície ampliada: `hw 256+k` vs `hw k` para anel, e `hw 240..255` vs `hw 0..15` para conteúdo real.
5. **Uma tela não representa o jogo.** Modo de vídeo e tamanho de mapa mudam entre título, gameplay e menu.

## Quando parar e pedir decisão humana

- O portão da seção 2 dá "não vale" mas alguém pediu ampliação mesmo assim: a decisão é de produto, não técnica. Registre a medição e escale.
- Seriam necessários mais de ~10 patches, ou patches em laço copiado para IWRAM: o caminho de bytes deixa de compensar ([ADR-043](../adr/ADR-043-rom-patch-at-load.md) §Critérios para revisitar).
- O patch exigiria condição de runtime (só no modo 0, só no nível 2): a v1 não tem condição declarativa.
- O perfil exigiria `status = "validated"`: isso exige relatório de cenários e versões, não basta o rótulo.
