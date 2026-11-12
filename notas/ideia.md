Inspiraçoes: 
Plymouth, Ly, Durdraw
---
Ideia:
O Plymouth e pesado e atrasa o boot, quero fazer algo mais simples que use ASCII arts animadas como bootsplash. Quando o Ly abrir ele avisa o RABA(Rust Ascii Boot Art), ele insterrompe a animaçao e abre o gerenciador de seçao.
---
Requisitos:
Ser leve, compativel com grub, limite, etc, agnostico, seguro, compativel com criptografia de disco, compativel com todos gerenciadores de seçao, agnostico, ser rapido, conseguir ler animaçoes personalizadas em arquivos .dur.
Feito em Rust, compativel com varias distros, open-source, disponivel no github com alguma licença copyleft, filosofia Unix, fazer uma unica coisa e fazer bem. Tem que ser estavel, no pior dos casos e quebrar nao podemos impedir o computador de dar boot, 
o programa tem que ter a menor pegada possivel tanto em armazenamento, quanto em RAM, CPU, zero GPU se possivel, como ele vai rodar no boot.
Se possivel ele ser compativel com systemd, mas nao ser obrigatorio, quem sabe o pessoal no voidlinux, artix, etc usem ele tambem. 
Quero disponibilizar no AUR quando der.
Pensei em ter um modo com 2 animaçoes, uma que fica tocando ate o sistema ficar protno e outra que toca uma vez depois do sistema ficar pronto que te levar para seu gerenciador de seçao. pensei em ser compativel com o Ly.
