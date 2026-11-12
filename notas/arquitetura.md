# RABA — Arquitetura, build e integrações

Decisões de projeto derivadas da pesquisa em `pesquisa.md`.
Status: rascunho de design, nada implementado ainda.

---

## 0. Resposta curta: o build se insere no GRUB?

**Não, e não deve.**

O Plymouth não faz isso no Arch — o wiki manda o usuário editar `/etc/default/grub` à mão
e adicionar `splash`. Razões para o RABA seguir o mesmo caminho:

1. **Padrão de empacotamento.** Pacotes não devem reescrever silenciosamente arquivos de
   outro pacote. `/etc/default/grub` pertence ao `grub`. O precedente no Arch é o bug
   FS#48324 (postfix), onde um `post_install` mexendo em `/etc` furou o tratamento de
   `.pacnew` do pacman. O canal correto é **mensagem no `.install`**.
2. **GRUB não é o único alvo.** Limine, systemd-boot, rEFInd, EFISTUB puro, e cada vez
   mais **UKI** (Unified Kernel Image, onde a cmdline vai embutida na imagem). Mexer só no
   GRUB atende uma fração dos usuários e ignora o requisito "agnóstico".
3. **Risco.** Um bug no nosso script de edição de bootloader = máquina que não dá boot.
   Isso viola o requisito mais importante do projeto.

**Solução:** comando opt-in `raba-setup`, que o usuário roda conscientemente, detecta o
bootloader e **mostra o que faria** antes de fazer (`--dry-run` por padrão). Mais mensagem
clara no `.install`.

### Mitigação: o RABA precisa mesmo da cmdline?

Parcialmente evitável. `quiet loglevel=3` existe pra impedir que `printk` escreva por cima
do desenho — mas o RABA pode **baixar o nível de printk ele mesmo** ao iniciar
(`/proc/sys/kernel/printk`, ou o ioctl `TIOCLINUX` subcódigo 11, que é o que o
`setterm --msg off` faz).

Isso cobre tudo a partir do momento em que o RABA sobe. Só as mensagens *anteriores* a ele
precisam do `quiet` na cmdline.

→ **Design: a cmdline é recomendada, não obrigatória.** Sem ela o splash funciona, só pode
ter lixo de kernel nos primeiros instantes. Isso é uma vantagem real de usabilidade sobre o
Plymouth, que simplesmente não aparece sem `splash` na cmdline.

---

## 1. Os três níveis de configuração

Essa é a parte que responde "build ou arquivo de config": **é um arquivo de config, mas
congelado na hora de gerar o initramfs.**

### Nível 1 — compilação (cargo features)
Só o que não dá pra decidir depois. Manter minimalista — evitar sopa de features.

```toml
[features]
default = ["backend-text"]
backend-text = []    # escape codes no VT (padrão)
backend-drm  = []    # DRM/KMS dumb buffer (opcional, futuro)
```

### Nível 2 — geração do initramfs (o que importa de verdade)
O hook lê `/etc/raba/raba.conf`, resolve o tema, e **copia pra dentro da imagem**:
- o binário
- o `.dur` escolhido (já resolvido, não os 15)
- uma cópia congelada da config relevante

```ini
# /etc/raba/raba.conf
#
# ATENÇÃO: alterações aqui só têm efeito depois de regenerar o initramfs.
# Use `raba-set-theme <nome>` que faz as duas coisas.

[Boot]
Theme=cachyos              # resolve pra /usr/share/raba/themes/cachyos.dur
                           # ou "auto" -> lê ID= de /etc/os-release no build
Tty=1
ShowDelay=0                # segundos antes de aparecer
Timeout=30                 # máx. em loop antes de desistir e liberar o VT

[Outro]
Enable=true
Theme=cachyos-outro        # animação de saída, toca 1x
MaxDuration=1.5            # teto duro. Sem isso, "o RABA atrasa o boot".

[Handoff]
Mode=retain                # retain | clear
```

Padrões do sistema em `/usr/share/raba/raba.defaults`, override do usuário em
`/etc/raba/raba.conf`. É o modelo do Plymouth (`plymouthd.defaults` +
`/etc/plymouth/plymouthd.conf`) e funciona bem — o pacote atualiza os defaults sem
gerar `.pacnew` no arquivo do usuário.

### Nível 3 — runtime (só kernel cmdline)
Dentro do initramfs o root **não está montado**, então `/etc/raba/raba.conf` do sistema
real não existe. Só dá pra ler a cmdline:

| parâmetro | efeito |
|---|---|
| `raba.enable=0` | kill switch total (espelha `plymouth.enable=0`) |
| `raba.debug=1` | log em `/run/raba.log`, não desenha |
| `raba.tty=N` | força o VT |

### ⚠️ Consequência que vai gerar bug report
**Mudar a config sem regenerar o initramfs não faz nada.** O Plymouth tem vários
relatos exatamente disso ("mudei `ShowDelay` e não aconteceu nada", "só funcionou depois
de `dracut -f`").

Mitigações obrigatórias:
- `raba-set-theme` regenera o initramfs sozinho (o `plymouth-set-default-theme -R` faz isso)
- comentário em letras garrafais no topo do `raba.conf`
- `raba-setup --check` compara o hash da config com o que está dentro do initramfs e avisa
  se estiver defasado

---

## 2. ⚠️ Problema novo: a fonte do console limita os glifos

**Isso contradiz a sugestão anterior de spinner braille — ela não funciona no console.**

O console do Linux é limitado a **256 ou 512 glifos** (limite de hardware VGA herdado).
Caracteres fora do conjunto carregado viram `?` ou vazio, e o valor Unicode original é
perdido pelo kernel.

Braille (U+2800–U+28FF) são 256 caracteres — cabem em 512 slots só sacrificando o bit de
brilho do foreground *e* 256 caracteres úteis. O Debian empacota isso separado
(`console-braille`), justamente porque não está na fonte padrão.

### Conjunto seguro de glifos
| categoria | exemplos | seguro? |
|---|---|---|
| ASCII 0x20–0x7E | `-` `.` `o` `+` `/` `:` `#` | ✅ sempre |
| blocos CP437 | `█ ▀ ▄ ▌ ▐ ░ ▒ ▓` | ✅ fonte padrão |
| box drawing | `─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼` | ✅ fonte padrão |
| Latin-1 acentuado | `á é ç ã` | ⚠️ depende da fonte |
| Braille | `⠋ ⠙ ⠹` | ❌ |
| setas, matemática, emoji | `→ ∑ 🐧` | ❌ |

**Spinner seguro:** `| / - \` (ASCII puro), ou blocos `▁▂▃▄▅▆▇█`, ou pontos `. .. ...`.

Os logos do fastfetch são quase todos ASCII puro (`-`, `.`, `o`, `+`, `/`, `:`, `s`), então
a base está salva. Alguns usam blocos, que também passam.

**Ação para o gerador:** validar cada glifo do tema contra o conjunto seguro e **falhar no
build** com a lista de caracteres problemáticos. Isso transforma um bug silencioso de
runtime (quadradinhos na tela de boot) num erro de compilação do tema.

---

## 3. ⚠️ Problema: HiDPI deixa o logo minúsculo

Fonte de console padrão é 8×16. Numa tela 4K isso dá 480×135 células — um logo de 20×20
fica do tamanho de uma unha.

O Plymouth resolve com `DeviceScale` porque desenha pixels. Em modo texto **não dá para
escalar** — a célula é o átomo.

Opções:
1. O hook injeta uma fonte grande (`ter-132b` do terminus, 16×32) via o hook `consolefont`
   do mkinitcpio. Resolve, mas vira dependência extra e altera o console pós-boot.
2. Oferecer variantes do tema em resoluções diferentes (`arch.dur`, `arch-2x.dur` com arte
   maior desenhada em blocos) e o hook escolhe pelo tamanho de tela detectado no build.
3. Aceitar e documentar.

Decisão provisória: **(3) na v1, (2) depois.** A (1) mexe demais no sistema do usuário.

---

## 4. Integrações — ranqueadas

### Essenciais (v1)
| integração | por quê |
|---|---|
| **mkinitcpio** (`install/raba` + `hooks/raba`) | Arch, CachyOS, Manjaro, EndeavourOS, Artix |
| **systemd units** (`raba-start`, `raba-quit`, `raba-quit-wait`) | espelhar o Plymouth exatamente, pro `ly@.service` funcionar sem patch |
| **handoff com o Ly** (drop-in `After=raba-quit-wait.service`) | o motivo do projeto existir |
| **`raba-ctl`** (cliente CLI via socket unix) | `raba-ctl quit`, `raba-ctl deactivate` — espelha o cliente `plymouth` |

### Importantes (v2)
| integração | por quê |
|---|---|
| **dracut** | Fedora, RHEL, openSUSE, e cada vez mais Debian |
| **initramfs-tools** | Debian/Ubuntu estável |
| **OpenRC / runit / s6 / dinit** | Void, Artix, Gentoo, Devuan — o requisito "não exigir systemd" |
| **agente de senha systemd** | LUKS com systemd (`/run/systemd/ask-password/`) |

### Depois
- **booster** (gerador de initramfs alternativo, popular no Arch)
- **UKI** — a cmdline vai embutida, então o `raba-setup` precisa saber mexer no
  `/etc/kernel/cmdline` em vez do bootloader
- **Secure Boot** — não exige nada especial (o binário vai no initramfs assinado), mas
  precisa ser verificado explicitamente

### Conflito a declarar
`conflicts=('plymouth')` no PKGBUILD, ou pelo menos detectar e avisar. Dois splashes
disputando o mesmo VT = tela preta ou travamento.

---

## 5. Problemas futuros previstos

Lista para virar issues/testes. Ordenada por quanto dói.

| # | problema | gravidade | mitigação |
|---|---|---|---|
| 1 | VT preso em `KD_GRAPHICS` → `VT_WAITACTIVE` trava o boot | 🔴 crítico | handler de panic restaura `KD_TEXT`; watchdog; teste dedicado |
| 2 | Hook falha e quebra a geração do initramfs | 🔴 crítico | hook nunca retorna erro; só avisa |
| 3 | Config alterada sem regenerar initramfs | 🟡 alto (suporte) | `raba-set-theme` automático; `--check` |
| 4 | Glifo fora da fonte do console | 🟡 alto | validação no gerador (§2) |
| 5 | Resolução muda no meio do boot (driver real de GPU carrega, fbcon redimensiona) | 🟡 alto | detectar `SIGWINCH`/consultar tamanho a cada frame e recentralizar |
| 6 | Ly sobe antes do `outro` terminar | 🟡 médio | `MaxDuration` + ordenação por `raba-quit-wait.service` |
| 7 | Console serial (`console=ttyS0`) | 🟡 médio | detectar e não desenhar; ou desenhar sem cor |
| 8 | `nomodeset` / sem fbcon / `CONFIG_VT=n` | 🟡 médio | detectar e sair com 0 silenciosamente |
| 9 | Prompt de LUKS colidindo com o desenho | 🟡 médio | v1: sair de cena (ver `pesquisa.md` §5) |
| 10 | `.dur` maior que a tela | 🟢 baixo | crop centralizado + aviso no log, nunca crashar |
| 11 | HiDPI (§3) | 🟢 baixo | documentar |
| 12 | `os-release` muda de `ID` numa atualização de distro | 🟢 baixo | `Theme=auto` re-resolve a cada regeneração do initramfs |
| 13 | Plymouth instalado junto | 🟢 baixo | `conflicts=` |

---

## 6. ⚑ Plano de testes em VM

**Decisão: todo teste de boot é feito em VM, nunca na máquina real.**
Motivo óbvio — o modo de falha do projeto é "a máquina não dá boot".

### Ferramenta
QEMU/KVM, scriptado. Nada de clicar em GUI.

```sh
qemu-system-x86_64 \
  -enable-kvm -m 2G \
  -bios /usr/share/edk2/x64/OVMF.4m.fd \   # UEFI
  -drive file=test.qcow2,if=virtio \
  -vga virtio \
  -serial stdio
```

`qcow2` + **snapshots**: tirar um snapshot do estado "limpo" e restaurar a cada teste.
Isso torna o ciclo de teste repetível e rápido.

### Matriz de testes
| eixo | variantes |
|---|---|
| firmware | UEFI (OVMF) / BIOS (SeaBIOS) |
| vídeo | `-vga virtio` (DRM ok) / `-vga std` / `-vga none` / `nomodeset` |
| init | systemd (Arch) / runit (Void) / OpenRC (Artix) / dinit |
| disco | sem cripto / LUKS |
| console | gráfico / `console=ttyS0` serial |
| initramfs | mkinitcpio / dracut / initramfs-tools |
| resolução | 1024×768 / 1920×1080 / 3840×2160 (teste de HiDPI) |

### Casos de falha a testar explicitamente
Mais importantes que os casos felizes:
- `.dur` corrompido / truncado / gzip inválido
- `.dur` v8 (que o Ly rejeita) e v7
- `.dur` maior que a tela
- `raba.enable=0`
- RABA morto com `SIGKILL` no meio da animação → o VT volta pro estado usável?
- Ly ausente / Ly falhando ao subir → cai num TTY utilizável?
- initramfs gerado sem o tema (arquivo faltando)

### Captura de resultado
- `-serial stdio` para capturar o log
- `qemu` monitor: `screendump arquivo.ppm` para capturar o framebuffer e comparar
- isso permite **teste de regressão visual automatizado** — comparar o ppm com um esperado

### Modo `--preview` (essencial pra velocidade de desenvolvimento)
Um modo que renderiza o `.dur` **no terminal atual**, sem tocar em VT nenhum:

```sh
raba --preview themes/arch.dur
```

Sem isso, iterar na arte exige um ciclo de boot de VM inteiro por tentativa. Com isso,
é instantâneo. **Implementar isso cedo, antes da integração com initramfs.**

---

## 7. Fluxo de build completo (visão geral)

```
┌─ build do pacote ────────────────────────────────────────────┐
│ cargo build --release --target x86_64-unknown-linux-musl     │
│   → raba, raba-ctl                                            │
│ raba-gen themes/*.toml                                        │
│   → /usr/share/raba/themes/*.dur   (valida glifos aqui!)      │
│ instala hooks, units, defaults                                │
└───────────────────────────────────────────────────────────────┘
                              ↓ pacman -S raba
┌─ pós-instalação ─────────────────────────────────────────────┐
│ .install imprime: "adicione 'raba' ao HOOKS em               │
│  mkinitcpio.conf (antes de 'encrypt'), rode raba-setup"      │
│ NÃO mexe no grub. NÃO mexe no mkinitcpio.conf.               │
└───────────────────────────────────────────────────────────────┘
                              ↓ raba-setup
┌─ geração do initramfs ───────────────────────────────────────┐
│ hook install/raba:                                            │
│   lê /etc/raba/raba.conf                                      │
│   resolve Theme (auto → ID de /etc/os-release)                │
│   copia binário + .dur + config congelada pra imagem          │
│   NUNCA retorna != 0                                          │
└───────────────────────────────────────────────────────────────┘
                              ↓ boot
┌─ runtime ────────────────────────────────────────────────────┐
│ hook runtime: lê cmdline, inicia raba no VT                   │
│ raba: baixa printk, pega VT, loop da animação 1               │
│ systemd: raba-quit-wait.service                               │
│ raba-ctl quit → animação 2 (1x, com teto) → restaura VT → 0   │
│ ly@tty2.service (After=raba-quit-wait.service)                │
└───────────────────────────────────────────────────────────────┘
```

---

## 8. Ordem de implementação sugerida

1. **Parser de `.dur`** (v7 + v8) + **`--preview`** no terminal. Zero integração com boot.
   Já é útil sozinho e permite iterar na arte.
2. **`raba-gen`** — logo fastfetch + spinner → `.dur`, com validação de glifos.
3. **Manejo de VT** — pegar VT, desenhar, restaurar. Testar o caso de falha #1 à exaustão
   em VM antes de qualquer outra coisa.
4. **Socket + `raba-ctl`** — máquina de estados BOOT → OUTRO → HANDOFF.
5. **Hook do mkinitcpio** + units systemd.
6. **Handoff com o Ly.**
7. Resto das integrações (dracut, initramfs-tools, inits não-systemd).
8. LUKS.

---

## 9. Camada de composição (overlay) — decisão

Surgiu da pergunta "dá pra mostrar barra de progresso?". A resposta expôs um limite do
modelo "RABA só toca `.dur`".

Um `.dur` é uma sequência fixa. Qualquer coisa que reaja a estado de runtime não cabe
dentro dele. Três requisitos caem nessa categoria:

| elemento | precisa de overlay |
|---|---|
| animação de fundo | não — é o `.dur` |
| prompt de senha LUKS | **sim** |
| `raba-ctl message` | **sim** |
| barra de progresso | **sim** |

Como o prompt de LUKS não é opcional, a camada de composição tem que existir de qualquer
jeito. A barra de progresso sai quase de graça depois dela.

**Regra que mantém o design limpo:**

> O `.dur` é o fundo. Overlays são desenhados por cima, pelo RABA, em resposta a estado
> de runtime. Um tema nunca contém overlay.

Consequências boas dessa fronteira: os temas continuam sendo dados puros, o `raba-gen`
continua validando estaticamente, e o compartilhamento de arte com o Ly continua
funcionando — o Ly desenha o mesmo fundo e simplesmente não tem overlays.

Detalhe de progresso: **não existe fonte confiável de progresso de boot no Linux.** Ver
`docs/progress.md`. Padrão = `pulse` (indeterminado, honesto); `raba-ctl progress` para
quem quiser alimentar valores reais. O caminho do Plymouth (cache de duração do boot
anterior em `/var/lib/`) é explicitamente **não recomendado** — `/var` pode não estar
montado, e o RABA nem chega a ver o root montado.

---

## 10. LUKS — decisão: ficar de fora

Revisão da seção 5 do `pesquisa.md`, depois de ler o código real do hook `encrypt`.

### Descoberta: o hook `encrypt` do mkinitcpio tem Plymouth hardcoded

```sh
if command -v plymouth >/dev/null 2>&1 && plymouth --ping 2>/dev/null; then
    plymouth ask-for-password --prompt="..." \
        --command="cryptsetup open --type luks --key-file=- ..."
else
    echo "A password is required to access the ${cryptname} volume:"
    while ! eval cryptsetup open --type luks "${resolved}" "${cryptname}"; do sleep 2; done
fi
```

Dois fatos:
1. **Procura um binário chamado `plymouth`, por nome.** Não existe interface genérica de
   splash. O RABA não consegue essa integração sem patch no mkinitcpio upstream, ou sem
   fingir ser o Plymouth.
2. **`--command=` com `--key-file=-` significa que o Plymouth segura o segredo** — ele lê
   as teclas, monta a senha e injeta no stdin do cryptsetup.

### O problema real do RABA é mais estreito

Não é "o RABA precisa pedir senha". É: se o RABA está desenhando no VT e o `cryptsetup`
pede senha no mesmo VT, os dois colidem.

**Evitar a colisão e coletar a senha são problemas separáveis.** Só o primeiro é nosso.

### Decisão

| nível | escopo | mexe no segredo | quando |
|---|---|---|---|
| **0** | hook **depois** do `encrypt` | não | **v1** |
| **1** | inotify em `/run/systemd/ask-password/` só pra se esconder | não | v2 |
| **2** | coletar a senha de fato | sim | só com motivo real |

Nível 0: a senha é pedida antes do RABA subir. Zero colisão, zero código, zero superfície
de segurança. Boot vira: firmware → prompt de senha → splash → DM.

Nível 1: o RABA usa o protocolo do systemd **só como aviso** de que alguém vai usar o
console. Nunca abre o socket. ~50 linhas.

Nível 2 é onde o projeto vira outra coisa — entra no caminho da chave do disco inteiro, e
passa a precisar de `mlock`, zeroização, buffer de capacidade fixa (uma `String` que
cresce realoca e deixa cópia na memória liberada), `PR_SET_DUMPABLE=0`, e nunca logar.
Em troca de a animação continuar rodando enquanto você digita. Não vale.

### ⚠ Correção ao que estava escrito antes

O `integration.md` dizia que o hook `raba` tinha que vir **antes** do `encrypt`. Errado —
era cópia do requisito do Plymouth, que vale porque o Plymouth *fornece* o prompt. Como o
RABA não fornece, ele vai **depois**. Corrigido em `docs/integration.md` e
`man/raba.conf.5.scd`.

### Nota sobre o requisito original

O `ideia.md` pede "compatível com criptografia de disco". O nível 0 já cumpre isso por
inteiro: a criptografia funciona, o boot funciona, o splash não atrapalha. "Compatível
com" não é "fornece o prompt de".

---

## 11. ⚠ Revisão da v1: não rodar no initramfs

Descoberta que muda o faseamento. A doc do systemd é direta sobre o `switch_root`:

> The switch-root operation will result in a **killing spree of all running processes**.

O Plymouth sobrevive, mas o maquinário não é pequeno: `plymouth-switch-root.service`
(oneshot `Before=initrd-switch-root.service` rodando `plymouth update-root-fs
--new-root-dir=/sysroot`), `KillMode=none`, `SendSIGKILL=no`, manter o daemon fora do
cgroup — **e um bug upstream conhecido** onde `/dev` ainda não está bind-montado em
`/sysroot/dev` na hora do re-root.

Tudo isso é específico de systemd. Para runit/OpenRC/s6/dinit teríamos que inventar o
equivalente — contra o requisito de ser agnóstico.

### Decisão: v1 roda **depois** do `switch_root`, como serviço comum

| | no initramfs | depois do switch_root |
|---|---|---|
| sobreviver ao kill | maquinário por init | não se aplica |
| colisão com LUKS | precisa tratar | impossível |
| hook de initramfs | obrigatório | nenhum |
| pode quebrar geração de imagem | **sim** | não |
| cobre | boot inteiro | boot pós-root |

Custo: splash começa mais tarde. Com `quiet loglevel=3` + `systemd.show_status=false` a
tela fica em branco/logo do firmware até lá.

Ganho: **some uma categoria inteira de falha catastrófica.** Sem hook de initramfs, não
existe a possibilidade de quebrar a geração de imagem num update de kernel — que é a pior
coisa que esse projeto poderia fazer com alguém.

E resolve o LUKS de graça: o RABA nem está rodando quando o `cryptsetup` pergunta.

**Faseamento revisado:**
- **v1** — serviço depois do `switch_root`. Zero envolvimento com initramfs.
- **v2** — hook opcional de initramfs, só systemd, com a dança do `switch_root`.
- **v3** — outros inits no initramfs, se alguém pedir.

Ver `docs/risks.md` §1.

---

## 12. ⚠ Revisão: formato próprio, handoff genérico, boot cedo

Três correções de direção. Elas revertem partes das seções 10 e 11.

### 12.1 Formato próprio (`.raba`), Durdraw vira importador

Regra que resolve a tensão:

> **Authoring time pode depender de qualquer coisa. Run time não depende de nada.**

O `raba-gen` converte de `.dur`, `.ans`, texto puro → `.raba`. Ele é ferramenta de build,
nunca entra no initramfs, nunca está no caminho de boot. Se o Durdraw mudar o formato,
quebra um *conversor* — erro de build que alguém conserta com calma. Nenhuma máquina
deixa de bootar.

Ganhos concretos, não só filosóficos:
- **A bomba de gzip deixa de existir** em vez de ser mitigada. O `.raba` declara o tamanho
  total no header, validado antes de qualquer alocação.
- `serde_json` e `flate2` somem do binário de boot. O orçamento de dependências do `raba`
  vira ~zero; o do `raba-gen` fica livre.
- Frames delta: spinner sobre logo estático custa algumas células por frame, não o canvas
  inteiro.
- Some a armadilha v7/v8 do colorMap no runtime — vira problema do conversor.

Ver `docs/format.md`.

### 12.2 Handoff genérico — nada de depender do Ly

A interface inteira é `raba-ctl quit`, mais `Boot.Timeout` como rede. O RABA não sabe o
que é um display manager, não lê arquivo de nenhum, e funciona igual com GDM, SDDM,
LightDM, greetd, Ly ou nenhum (getty).

Transição sem costura passa a ser **propriedade da arte**, não integração: o último frame
do outro bate com o que vem depois. Com `Handoff.Mode=retain` o frame simplesmente fica na
tela, o que já cobre a maioria dos casos sem coordenação nenhuma.

O Ly tocar `.dur` vira curiosidade útil (o `raba-gen --also-dur` existe pra isso), não
arquitetura.

### 12.3 Começar cedo — e a §11 estava errada

A §11 propunha rodar depois do `switch_root` pra fugir da matança de processos. Trocava
cedo demais por simplicidade. Revertido.

**Solução melhor: ser retomável.** O estado é minúsculo (VT, índice do frame, fase,
timestamp, caminho do tema) e vai pra `/run/raba.state` a cada frame — `/run` atravessa o
`switch_root`.

Um único caminho de código trata os dois casos:

```
start
  ├─ state fresco existe ──► retoma: adota o VT, continua do frame
  └─ sem state ────────────► começa do zero: pega VT, frame 0
```

Sobreviver ao `switch_root` vira **otimização**, não requisito:
- sobreviveu → o loop continua, de graça
- foi morto → a próxima instância retoma no mesmo VT e no mesmo frame; a tela segurou o
  último frame nesse meio tempo, porque ninguém limpou

E o mesmo mecanismo absorve OOM kill, crash, `SIGKILL` de um init que não previmos.
**Ser morto deixa de ser evento.** Pra um programa cujo primeiro requisito é nunca
quebrar boot, isso vale mais que o caminho limpo de sobrevivência do Plymouth — e, ao
contrário dele, não é específico de systemd.

### 12.4 LUKS volta à mesa (porque agora começamos antes do `encrypt`)

Continua valendo: o RABA **não coleta senha**. Só precisa sair da frente.
- systemd: inotify em `/run/systemd/ask-password/`, só como aviso. Nunca abre o socket.
- busybox: detectar `cryptsetup` rodando via `/proc`. Grosseiro, mas falha benigna — se
  não detectar, desenha por cima: feio e ainda boota.

Colocar o hook depois do `encrypt` continua suportado pra quem prefere certeza.

Ver `docs/early-boot.md`.

### 12.5 O bootloader pode cobrir antes de nós

O GRUB mostra imagem de fundo. Se for o **primeiro frame** da animação, a tela já está
certa desde o bootloader e o RABA só assume dali. É um começo genuinamente mais cedo do
que qualquer programa de userspace alcança, e custa um PNG. O `raba-gen` pode emitir o
primeiro frame como imagem.

Opcional, degrada pra "splash começa um pouco depois" se não usar.
