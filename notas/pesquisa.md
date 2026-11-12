# RABA — Pesquisa técnica

Notas de pesquisa sobre as peças mencionadas em `ideia.md`.
Data: 2026-11-12.

---

## 1. Formato `.dur` (Durdraw)

**O que é:** um JSON gzipado. Objeto raiz `DurMovie`.

Versão atual: **8** (Durdraw 0.30.0, agosto/2026). Anterior: **7** (maio/2023, primeira
versão pública JSON/GZIP).

### Chaves do `DurMovie`
| chave | significado |
|---|---|
| `formatVersion` | versão do formato (7 ou 8) |
| `colorFormat` | `"16"`, `"256"`, `"RGB"`, `"mIRC"` |
| `preferredFont` | opcional |
| `encoding` | `"utf-8"` ou `"cp437"` |
| `name`, `artist` | metadados (geram registro SAUCE) |
| `framerate` | FPS de reprodução (float) |
| `columns` / `lines` | tamanho do canvas (v8) |
| `sizeX` / `sizeY` | idem, nomes da v7 |
| `extra` | JSON livre do usuário, ignorado pelo Durdraw |
| `frames` | array de frames |

### Frame
- `frameNumber` (int)
- `delay` (float, **segundos**) — tempo parado nesse frame; `0` = usa o `framerate` global
- `contents` — array de strings, uma por linha do canvas
- `colorMap` — array de pares `[fg, bg]` por célula

### Diferença v7 → v8 (importante)
- v7: `sizeX`/`sizeY`; e o `colorMap` é **transposto** — `contents[linha][coluna]`
  corresponde a `colorMap[coluna][linha]`.
- v8: `columns`/`lines`; `colorMap[linha][coluna]` bate direto com `contents`.

Isso é uma pegadinha real: um parser que ignore a versão vai renderizar cores embaralhadas.

### Paleta
- modo 256: índices 0–15 são as cores ANSI clássicas (0=preto … 15=branco brilhante),
  16–255 = paleta xterm-256color padrão.
- modo 16: os índices começam em **1** em vez de 0 (1=preto … 16=branco brilhante).

Spec oficial: `durformat.md` no repo do Durdraw.

---

## 2. Ly — descoberta mais importante da pesquisa

**O Ly já toca arquivos `.dur` nativamente.** Isso muda o desenho do projeto.

No `res/config.lua` do Ly (master):
```lua
animation = "none",   -- opções incluem "dur" e "lua"
animation_frame_delay = 5,
animation_timeout_sec = 0,
dur_file_path = "$CONFIG_DIRECTORY/ly/example.dur",
dur_offset_alignment = "center",
dur_x_offset = 0,
dur_y_offset = 0,
lua_animation_file = "$CONFIG_DIRECTORY/ly/example.lua",
```

Implementação: `src/animations/DurFile.zig`. Outras animações: Doom (fogo do PSX),
Matrix, ColorMix, GameOfLife, Cascade, e animações customizadas em **LuaJIT**.

### Limitações do parser do Ly (restringem o que o RABA deve gerar/aceitar)
Lendo o `validate()` do `DurFile.zig`:
- aceita **somente `formatVersion == 7`** — v8 é rejeitada com `UnsupportedVersion`
- `colorFormat` só `"16"` ou `"256"`
- `encoding` só `"utf-8"`
- `framerate` tem que existir e ser > 0

→ Para compatibilidade total RABA↔Ly, o denominador comum hoje é **v7 / 256 cores / utf-8**.
O RABA deve ler v7 **e** v8 (tratando a transposição do colorMap), mas recomendar v7 na doc.

### Estado do projeto Ly
- escrito em **Zig** (0.15.x / 0.16.x dependendo da branch)
- **desenvolvimento migrou para o Codeberg**; o GitHub virou espelho. Mirar no Codeberg.
- não exige systemd; tem scripts de serviço para systemd, OpenRC, runit, s6, dinit, sysvinit
- `ly@.service` é instanciado por tty, roda via `agetty -nl`, com
  `Alias=autovt@.service`, `DefaultInstance=tty2`

### O "Ly avisa o RABA" — isso NÃO existe hoje
O Ly não tem IPC, socket nem hook. Mas o `ly@.service` já traz:
```
After=systemd-user-sessions.service plymouth-quit-wait.service getty@%i.service
```
Ou seja: o Ly **já espera** o `plymouth-quit-wait.service`. Três caminhos:

1. **Sem tocar no Ly (recomendado p/ v1):** o RABA espelha o modelo do Plymouth —
   `raba-quit.service` + `raba-quit-wait.service`, e um drop-in em `ly@.service`
   com `After=raba-quit-wait.service`. Zero patch, funciona hoje.
2. **Wrapper:** `ExecStartPre=/usr/bin/raba-ctl quit` no drop-in do serviço do Ly.
   Funciona igual em OpenRC/runit/s6/dinit, só editando o script de serviço.
3. **PR no Ly (v2):** adicionar um `raba_quit = true` ou um hook genérico de pré-start.
   Vale abrir a issue cedo — o pessoal do Ly claramente gosta de ASCII, já que
   implementaram o `.dur`.

### Transição "sem costura" — como o Plymouth faz
A sequência que GDM/SDDM usam é: `plymouth deactivate` → o DM assume a tela →
`plymouth quit --retain-splash` (mantém o conteúdo na tela em vez de limpar).
O `--retain-splash` deixa o VT em modo gráfico, e o DM precisa iniciar no VT ativo.
Distros evitam usar isso em scripts genéricos: se o DM falhar, o splash fica preso
na tela escondendo o prompt de login texto. **Lição para o RABA: sempre ter timeout e
fallback que devolve o VT para modo texto.**

---

## 3. Renderização: dois caminhos possíveis

### (A) Texto no VT via escape codes — recomendado
Abrir `/dev/ttyN`, manter `KD_TEXT`, escrever ANSI/VT100 direto. O `fbcon` do kernel
desenha.

- **Prós:** minúsculo, zero GPU, zero dependência de driver DRM, não aumenta o initramfs,
  funciona em qualquer máquina que tenha console. Casa perfeitamente com "ASCII art" e com
  o Ly (que também desenha num terminal) → transição realmente contínua.
- **Contras:** depende de `CONFIG_VT`/`fbcon` compilado; mensagens de `printk` escrevem
  por cima; a grade de células é ditada pela fonte do console (sem controle sub-célula);
  resolução muda quando o driver real de vídeo carrega (flicker/resize).

**Silenciar o kernel por cima do desenho:**
- cmdline: `quiet loglevel=3` — e nessa ordem, `quiet` primeiro
- `setterm --msg off` (só vale em console virtual) ou o ioctl `TIOCLINUX` equivalente
- `sysctl kernel.printk = 3 3 3 3`
- `systemd.show_status=false` para as mensagens do systemd

### (B) DRM/KMS com dumb buffer + fonte bitmap própria
É o que o Plymouth faz. Controle total do pixel, funciona em `KD_GRAPHICS`, independe
do fbcon.

- **Contras:** precisa dos módulos DRM no initramfs (é exatamente isso que **dobra** o
  tamanho do initrd do Plymouth), precisa lidar com simpledrm vs driver real, hotplug,
  multi-monitor. Muito mais código.
- Crates: `drm` (drm-rs — ioctls, modesetting, dumb buffers), `softbuffer` (backend KMS
  em cima do `drm`, mas puxa `std` + `raw_window_handle`), `linux-drm` (sem libc, mas só
  em nightly). Nenhuma é `no_std`.

**Veredito:** fazer (A) como núcleo. Deixar (B) como backend opcional atrás de uma
feature flag, se algum dia for necessário em hardware sem fbcon.

### A dança do VT — referência de implementação
O `psplash` do Yocto (`psplash-console.c`) é o melhor modelo mínimo em C:
1. configurar troca de VT por sinal (`VT_PROCESS`)
2. `VT_OPENQRY` → achar um VT livre
3. `VT_ACTIVATE` + `VT_WAITACTIVE`
4. `KDSETMODE` para `KD_GRAPHICS` (só no caminho B)
5. **na saída:** voltar a `KD_TEXT` e reativar o VT original

**Armadilhas documentadas:**
- se o VT ficar preso em `KD_GRAPHICS`, `VT_ACTIVATE` falha silenciosamente e
  `VT_WAITACTIVE` **trava no boot** (aconteceu com o Plymouth, reportado na lista do Xorg)
- depois que um programa em `KD_GRAPHICS` sai, o splash precisa ser redesenhado inteiro
- `/dev/tty0` == `/dev/console` (o VT atual), não um VT específico. Os VTs são `/dev/ttyN`, N≥1.

---

## 4. Boot cedo: initramfs

Para aparecer antes do root montar (obrigatório se houver LUKS), o binário precisa estar
**dentro do initramfs**.

- **Arch/mkinitcpio:** hook `plymouth` no `HOOKS=`; com initramfs baseado em systemd é
  `sd-plymouth`. O hook tem que vir **antes** do `encrypt`. Rebuild com `mkinitcpio -P`.
  O hook `kms` é padrão desde o mkinitcpio v33 para drivers in-tree.
- **Debian/Ubuntu:** `initramfs-tools` com `FRAMEBUFFER=y` num `conf.d`.
- **Fedora/dracut:** módulo dracut.
- Cmdline: o parâmetro `splash` é só convenção consumida pelo userspace — quem lê é o
  splash, não o kernel.

**Vantagem do caminho (A):** sem precisar de módulos DRM no initrd, o RABA aumenta o
initramfs em ~o tamanho do binário, não em dezenas de MB. Esse é o argumento de venda real.

Variável relacionada: `CONFIG_FRAMEBUFFER_CONSOLE_DEFERRED_TAKEOVER=y` mantém o logo do
firmware até alguém desenhar (usado pelo `fbspinner`).

---

## 5. Criptografia de disco (LUKS) — a parte mais difícil

Dois mundos completamente diferentes:

### Com systemd
Existe um protocolo documentado: **systemd Password Agents** (`systemd.io/PASSWORD_AGENTS`).
- quem pergunta cria um arquivo `ask.xxxx` em `/run/systemd/ask-password/` no formato ini,
  seção `[Ask]`, com `Message=`, `PID=`, `Icon=`, `Echo=`, `NotAfter=`, `Socket=`
- o **agente** (seria o RABA) observa o diretório com `inotify`, mostra o prompt, e
  responde num socket `AF_UNIX`/`SOCK_DGRAM`
- vários agentes podem rodar ao mesmo tempo; o primeiro que responder ganha
- esconder o diálogo quando o arquivo some ou `NotAfter=` expira
- acesso ao socket é restrito a usuário privilegiado
- desde o systemd v257 há também um diretório por usuário em `$XDG_RUNTIME_DIR`
- o Plymouth entra nisso via `systemd-ask-password-plymouth.service`, que roda
  `systemd-tty-ask-password-agent --watch --plymouth`, só inicia se o pidfile do Plymouth
  existir, e é pulado com `plymouth.enable=0` na cmdline

### Sem systemd (mkinitcpio busybox, void, artix…)
O hook `encrypt` chama o `cryptsetup`, que lê a senha **direto do console**. Não há
protocolo. O RABA precisa ou ceder o VT enquanto o cryptsetup pergunta, ou ser invocado
pelo hook para fazer o prompt e repassar a senha (é o que o Plymouth faz com
`plymouth ask-for-password --prompt=...`).

**Recomendação:** v1 = detectar que vai haver prompt e **sair de cena** (restaurar VT,
deixar o cryptsetup perguntar normalmente). Feio, mas nunca quebra o boot.
v2 = agente systemd. v3 = prompt próprio no caminho não-systemd.

Bugs conhecidos do Plymouth aqui (evitar repetir): senha errada volta pra mesma tela sem
mensagem de erro; `plymouth ask-for-password` com saída redirecionada devolve senha vazia.

---

## 6. "Nunca impedir o boot" — regras de projeto

Isso precisa virar requisito testado, não intenção:

- **kill switch na cmdline:** `raba.enable=0` (o Plymouth tem `plymouth.enable=0`)
- **nunca** colocar o RABA como `Requires=`/dependência dura de nada no caminho crítico
- `TimeoutStartSec=` curto; watchdog interno que mata a animação e devolve o VT
- `panic = "abort"` + um handler que restaura `KD_TEXT` e o VT original antes de morrer;
  sair sempre com código 0
- o hook de initramfs **nunca** pode falhar o build da imagem
- testar explicitamente: VT preso em `KD_GRAPHICS` (o travamento do `VT_WAITACTIVE`),
  `.dur` corrompido, `.dur` maior que a tela, DM que não sobe

---

## 7. Bootloaders (GRUB, Limine, systemd-boot, rEFInd)

Quase irrelevante para o desenho. O que importa:
- o bootloader passa a **cmdline** do kernel (onde vão `quiet loglevel=3` e os `raba.*`)
- o bootloader/firmware entrega um framebuffer usável (EFI GOP → `efifb`/`simpledrm`)

Todos os citados fazem isso. Não há integração específica a escrever — só documentar
como adicionar os parâmetros em cada um.

Sobre `simpledrm`: em UEFI, o Plymouth pode usar o driver simpledrm no framebuffer do
firmware. Isso faz o splash aparecer em máquinas onde o driver da GPU demora (ex.: AMD,
onde o `amdgpu` pode carregar 10–16s depois do início do boot) e elimina o flicker entre
a tela do firmware e o splash. O Fedora tornou isso padrão.

---

## 8. "O Plymouth é pesado e atrasa o boot" — nuance

Vale calibrar a premissa antes de usar como argumento de marketing:

- o `plymouth-quit-wait.service` aparece gigante no `systemd-analyze blame`, mas ele
  **continua rodando depois que a tela de login já apareceu** — ele espera o DM mandar o
  Plymouth sair. Mantenedores do Fedora mediram ~**3,5s reais** contra os **8,3s** que o
  `systemd-analyze` reportava (RHBZ 1795021).
- um usuário do Ubuntu mediu ~10s de diferença no `systemd-analyze` e **zero** diferença
  no tempo até o ambiente gráfico subir.
- mas atrasos reais existem em casos específicos: >5min num VM RHEL 10, ~20s num kernel
  do Manjaro.

**Conclusão honesta:** o custo real do Plymouth é (a) **tamanho do initramfs** — os
módulos DRM dobram a imagem — e (b) a dependência de o driver de GPU carregar cedo, que
causa splash atrasado ou ausente. É aí que o RABA ganha, não em "o Plymouth é lento".
Usar essa linha no README é mais defensável e não vai ser contestado.

---

## 9. Arte anterior (vale olhar o código)

| projeto | linguagem | o que é |
|---|---|---|
| **psplash** (Yocto) | C | splash mínimo de framebuffer. **Melhor referência para o manejo de VT.** |
| **fbspinner** | Rust | spinner no framebuffer durante o boot; usa `CONFIG_FRAMEBUFFER_CONSOLE_DEFERRED_TAKEOVER` |
| **EasySplash** | Rust | splash para Linux embarcado, mas toca **vídeo** via GStreamer |
| **fbsplash / splashutils** | C | o splash legado do Gentoo, pré-Plymouth |
| **mkinitcpio-archlogo** | — | logo ASCII colorido no early boot do Arch. Prova de conceito do nicho. |
| **Plymouth** | C | referência de arquitetura: plugins de renderer (drm, framebuffer, text), socket de IPC, cliente CLI |

**Não existe um splash de boot ASCII em Rust.** O nicho é real.

Nota: o postmarketOS *saiu* do pbsplash *para* o Plymouth em abril/2026 — vale entender
o porquê antes de assumir que o caminho leve é obviamente melhor para todos os casos.

---

## 10. Crates Rust sugeridas

- `rustix` — ioctls, termios, VT, sem depender de libc em C. Ideal para binário estático musl.
  (`nix` é a alternativa mais conhecida, mas puxa libc.)
- `flate2` (backend `miniz_oxide`, puro Rust) — descompactar o gzip do `.dur`
- JSON: `serde_json` é o óbvio, mas é o que mais engorda o binário. Para um `.dur`, a
  estrutura é fixa e simples — um parser manual ou `tinyjson`/`nanoserde` corta bastante.
- **evitar:** `tokio`/async (desnecessário para um loop de frames), `drm`/`softbuffer`
  no caminho (A)
- perfil de release: `opt-level = "z"`, `lto = true`, `codegen-units = 1`,
  `panic = "abort"`, `strip = true`, target `x86_64-unknown-linux-musl`

---

## 11. Modo de duas animações — máquina de estados

Mapeia direto no que foi descrito:

```
BOOT    → toca animação 1 em loop
          (+ eventualmente: prompt de senha LUKS)
READY   → sinal externo (raba-ctl quit / socket / unit systemd)
OUTRO   → toca animação 2 uma única vez, até o fim
HANDOFF → restaura VT, sai com 0, o Ly assume
```

Detalhe de design: se o sinal READY chegar no meio do loop, esperar o frame atual terminar
antes de cortar — senão o corte fica feio. E `OUTRO` precisa de timeout máximo, senão uma
animação longa vira "o RABA atrasa o boot" (exatamente a crítica feita ao Plymouth).

Bônus: se o último frame do `OUTRO` for igual ao primeiro frame do `.dur` configurado no
Ly, a transição fica literalmente invisível.

---

## 12. Licença e nome

**Licença copyleft:**
- `GPL-3.0-or-later` — copyleft forte, escolha padrão, combina com "filosofia Unix / open source"
- `MPL-2.0` — copyleft fraco, por arquivo; se um dia alguém quiser embutir o RABA num
  produto, isso facilita adoção
- Recomendação: **GPL-3.0-or-later**, já que é um utilitário standalone e não uma biblioteca.

**Nome:** vale notar conscientemente que "raba" é gíria em português. O acrônimo
(Rust ASCII Boot Art) funciona, e nomes de projeto brincalhões não são incomuns no Linux —
só é bom ser uma decisão, não um acidente, já que a ideia é publicar no AUR e no GitHub.

---

## 13. Resumo das decisões que a pesquisa sugere

| questão | sugestão |
|---|---|
| renderização | texto no VT via escape codes (fbcon), DRM como backend opcional futuro |
| formato | ler `.dur` v7 **e** v8, cuidando da transposição do colorMap; recomendar v7 |
| integração com Ly | `raba-quit-wait.service` espelhando o Plymouth — zero patch no Ly |
| LUKS v1 | sair de cena e deixar o cryptsetup perguntar |
| LUKS v2 | agente de senha do systemd |
| init | binário agnóstico + scripts separados para systemd/OpenRC/runit/s6/dinit |
| segurança | `raba.enable=0`, timeout, panic=abort com restauração de VT, saída sempre 0 |
| licença | GPL-3.0-or-later |
| repo alvo p/ integração | Codeberg (Ly migrou pra lá) |

---

## Fontes

- Durdraw: https://github.com/cmang/durdraw — spec `durformat.md`
- Ly (upstream): https://codeberg.org/fairyglade/ly — espelho: https://github.com/fairyglade/ly
- Ly `DurFile.zig`: https://github.com/fairyglade/ly/blob/master/src/animations/DurFile.zig
- Ly `config.lua`: https://github.com/fairyglade/ly/blob/master/res/config.lua
- systemd Password Agents: https://systemd.io/PASSWORD_AGENTS/
- `systemd-ask-password(1)`: https://www.freedesktop.org/software/systemd/man/253/systemd-ask-password.html
- Plymouth (freedesktop): https://wiki.freedesktop.org/www/Software/Plymouth/
- Arch Wiki — Plymouth: https://wiki.archlinux.org/title/Plymouth
- Arch Wiki — Silent boot: https://wiki.archlinux.org/title/Silent_boot
- Gentoo Wiki — Plymouth: https://wiki.gentoo.org/wiki/Plymouth
- Gentoo Wiki — Fbsplash: https://wiki.gentoo.org/wiki/Fbsplash
- Fedora — Plymouth usa simpledrm: https://www.fedoraproject.org/wiki/Changes/PlymouthUseSimpledrm
- psplash (manejo de VT): https://git.yoctoproject.org/psplash/tree/psplash-console.c
- `ioctl_console(2)`: https://www.man7.org/linux/man-pages/man2/ioctl_console.2.html
- VT em KD_GRAPHICS trava VT_WAITACTIVE: https://lists.freedesktop.org/archives/xorg-devel/2014-August/043563.html
- `plymouth(1)` — `quit --retain-splash`: https://man.archlinux.org/man/plymouth.1.en
- SDDM, transição suave: https://github.com/sddm/sddm/pull/1932
- RHBZ 1795021 — plymouth-quit-wait e tempo de boot: https://bugzilla.redhat.com/show_bug.cgi?id=1795021
- Ubuntu Discourse — medição do Plymouth: https://discourse.ubuntu.com/t/disable-plymouth-for-smoother-faster-default-boot/3527
- crate `drm` (drm-rs): https://docs.rs/crate/drm/0.14.2/source/README.md
- EasySplash: https://github.com/OSSystems/EasySplash
- fbspinner: https://libraries.io/cargo/fbspinner
- postmarketOS, pbsplash → Plymouth: https://postmarketos.org/edge/2026/04/30/Switching-from-pbsplash-to-Plymouth/
