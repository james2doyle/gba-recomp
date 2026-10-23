# Verificação manual — localização (SPEC-069)

Os testes automatizados cobrem o modelo (menu, detecção, atlas, gesto) e o desenho até o
backbuffer. **Não** cobrem janela real, aparelho Android, pilha WebAssembly, nem o
comportamento do sistema operacional. Esses itens só valem como verificados depois de
executados numa máquina/aparelho real, com o resultado registrado.

Nada abaixo foi executado neste ambiente: não há dispositivo, emulador, `emcc`, nem servidor
de sessão gráfica.

## Pré-requisitos

- `games/rkanoid-v3.00.gba` (homebrew livre, já no repositório).
- Build: `cargo build -p gba-desktop -p gba-settings -p gba-party`
  Para testar a partir de um pacote, construa os companions também: `cargo build --release -p gba-recomp -p gba-native-rt -p gba-settings -p gba-party`. Sem os `-p` dos companions, o pacote leva binários antigos (o empacotador avisa).
- Para começar de um estado limpo, remova `~/.config/gba-recomp/settings.toml`.

## 1. Menu na janela — desktop

1. `target/debug/gba-desktop --rom games/rkanoid-v3.00.gba`
2. Abra o menu com `Esc` e com `F1`.
3. **Conferir**: título "Menu"; linhas "Continuar", "Vídeo", "Áudio", "Controles",
   "Janela de configurações...", "Reiniciar jogo", "Salvar agora", "Sair", "Idioma".
4. **Conferir os acentos**, que antes eram impossíveis: "Vídeo", "Áudio",
   "Configurações", "Latência", "Correção de cor", "Nítido", "Não".
5. **Conferir o alinhamento**: os valores à direita (`3x`, `Não`, `Nítido`, `100%`,
   `60 ms`) encostam na margem direita sem encostar no rótulo.
6. Entre em Vídeo, Áudio e Controles e percorra todas as linhas. **Conferir** que nenhuma
   linha é cortada embaixo nem colide com o rodapé.
7. **Conferir** o rodapé: "Setas/Enter/Esc  ou  D-pad/A/B".

## 2. Troca de idioma em tempo real

Há dois idiomas: `pt-BR` (padrão) e `en-US`.

1. No menu principal, vá até "Idioma" e pressione `Direita`.
2. **Conferir**: a linha mostra "English (US)" e **o menu inteiro muda na hora** — título, todas
   as linhas e o rodapé passam a "Menu", "Resume", "Video", "Settings window...", "Quit",
   "Language", "Arrows/Enter/Esc or D-pad/A/B". O menu não pisca, não fecha e o cursor
   continua na mesma linha.
3. Entre em Controles: **conferir** "Right keyboard", "Right gamepad", "Select keyboard",
   "Up gamepad" — o nome do botão é traduzido antes de entrar na linha.
4. Pressione `Esquerda`: volta a "Português (Brasil)" e todos os textos voltam ao português.
   `Direita` a partir do último idioma e `Esquerda` a partir do primeiro dão a volta.
5. **Conferir a persistência**: saia do jogo rodando de novo; o menu deve abrir já em inglês
   (há `language = "en-US"` em `settings.toml`).
6. Apague a linha `language` do `settings.toml` e repita: volta a seguir o idioma do sistema,
   ou `pt-BR` se não houver tradução para ele.

## 2b. Detecção do idioma do sistema

1. Remova `settings.toml` e rode com `LANG=en_US.UTF-8`: a interface abre em inglês.
2. Idem com `LANG=ja_JP.UTF-8`: abre em português (sem tradução para japonês).
3. Idem com `LANG=en_GB.UTF-8`: abre em inglês, apesar de não existir variante `en-GB`.
4. Escolha português no menu, saia e mude o `LANG` para `en_US`: **deve** continuar em
   português — a escolha explícita vence o sistema, inclusive quando ele muda.
5. No macOS, mude o idioma do sistema com o jogo aberto: sem escolha salva, o menu deve
   acompanhar (`SDL_EVENT_LOCALE_CHANGED`).

## 3. Janela de configurações (`gba-settings`)

1. `target/debug/gba-settings --page video` (ou pelo menu: "Janela de configurações...").
2. **Conferir**: o seletor "Idioma" aparece no topo da aba Vídeo, antes dos demais campos.
3. Troque o idioma e **conferir** que a janela se redesenha na hora.
4. Feche; o jogo (rodando) deve recarregar o `settings.toml` em até ~0,5 s e usar o novo
   idioma no menu. Enquanto isso, o `stderr` do jogo mostra `language changed to en-US`.
5. **No sentido inverso**: com a janela de configurações aberta, troque o idioma no menu do
   jogo. A janela aberta deve se redesenhar em inglês em até ~1 s, sem fechar e sem perder
   um valor que você tenha mexido e não aplicado (o idioma muda; o resto do rascunho fica).
5. **Conferir** os rótulos traduzidos: "Escala da janela", "Tela cheia", "Filtro",
   "Nítido (nearest)", "Suave (linear)", "Escala inteira", "VSync", "Correção de cor do
   LCD", "Volume", "Mudo", "Latência máxima", e a nota "Estas opções só mudam a
   apresentação; o jogo roda igual (SET-63-02)."
6. Com inglês escolhido, reabrir a janela: **deve** abrir já em inglês, com o título
   "gba-recomp settings" e as abas "Video/Audio/Controls/GPIO", os botões "Apply/Cancel/
   Restore defaults for this tab" e a dica "unsaved changes". Conferir também a aba GPIO
   ("Use GPIO buttons", "Chip", "Button wired to GND (active low)", "Debounce",
   "Test pins live ...") e a lista de dispositivos ("wired (USB)"/"wireless (Bluetooth)").

## 4. Menu de contexto nativo e barra de menu (macOS)

1. Clique com o botão direito no jogo.
2. **Conferir** submenus "Vídeo", "Som", "Controles", "Sistema" e as ações em português
   acentuado, incluindo "Correção de cor do LCD".
3. **Conferir** que "Pausar"/"Continuar" troca de verbo conforme o estado.
4. Na barra de menus, **conferir** "Sistema" e "Menu na janela...".
5. **Conferir o submenu "Idioma"** no menu de contexto: lista "Português (Brasil)" e
   "English (US)", com marca no corrente. Escolher inglês: o menu fecha, o jogo redesenha em
   inglês e a escolha é gravada em `settings.toml`.
6. Com inglês escolhido, **conferir** a barra de menus: "System", "In-window menu...", e as
   entradas do menu do aplicativo ("Settings...", "Party over network...").

No Linux não há adaptador nativo: o botão direito deve abrir o menu da janela (checar que
continua assim).

## 5. Pressão longa (toque) — **requer aparelho/emulador Android ou Web móvel**

1. Abra o jogo com controles na tela visíveis.
2. (a) Segure o direcional por 1 s: **não** deve abrir o menu; o personagem deve andar.
   (b) Segure A por 1 s: **não** deve abrir o menu.
   (c) Solte e dê um toque curto: nada deve abrir.
3. Segure numa área livre da tela (fora do direcional e dos botões) por ~0,5 s:
   **deve** abrir o menu.
4. Repita segurando e arrastando: **não** deve abrir (é arrasto, não pressão).
5. Com o menu aberto, toques não devem reabrir o menu nem travá-lo; saia com `Esc`/`B`.

## 6. Botão voltar (Android) — **requer aparelho/emulador**

1. Com o jogo rodando, pressione voltar: **deve** abrir o menu, **não** fechar o app.
2. Dentro do menu, pressione voltar: **deve** voltar/fechar o menu.
3. Em Controles, inicie um remapeamento de teclado e pressione voltar: **deve** cancelar o
   remapeamento, nunca gravar "AC Back" como binding.

## 7. Web (WebAssembly) — **requer `emcc`**

1. `gba-recomp bundle` do rkanoid para o alvo Web e sirva a página.
2. **Conferir** que o menu abre por `F1` e que o texto aparece acentuado.
3. Em janela móvel do navegador, repetir os casos de pressão longa do item 5.
4. **Conferir** que o menu nativo do navegador (callout/seleção ao segurar) **não** aparece
   sobre o canvas.
5. **Conferir** que o idioma escolhido sobrevive a recarregar a página (IndexedDB).

## 7b. Party (`gba-party`)

1. Abra pelo jogo (menu → "Party em rede...") com inglês escolhido.
2. **Conferir**: título "Party over network", "Your name", "Host a room", "Create room",
   "Join a room", "Join", e as mensagens do log ("Connected.", "%s joined the room.").
3. Entre numa sala: "Code:", "Address:", "Copy invite", "Participants", "Message", "Send",
   "Leave the room".

## 8. CLI

1. `LANG=pt_BR.UTF-8 target/debug/gba-desktop --bogus`
   → "argumento desconhecido `--bogus`" seguido do uso em português.
2. `LANG=en_US.UTF-8 target/debug/gba-desktop --help`
   → "usage: gba-desktop ..." e "Esc/F1 or right click: menu", sem nenhum texto em português.
3. `LANG=ja_JP.UTF-8 target/debug/gba-desktop --help` e sem `LANG` → uso em português.
4. `LANG=en_GB.UTF-8 target/debug/gba-desktop --help` → uso em inglês.

## Como adicionar um idioma

Ver [adding-a-language.md](adding-a-language.md). É o roteiro de contribuição
completo, com as armadilhas de TOML, interpolação e cobertura de fonte.

## Registro

Ao concluir, registre em `knowledge/` ou no ROADMAP quais itens foram executados, em qual
máquina/aparelho, e o resultado observado — incluindo o que não passou. Um item não
executado permanece **não verificado**; não o marque como aprovado por inferência.
