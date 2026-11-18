# BlueBl

Chat **malla por Bluetooth LE** en la terminal: sin internet, sin servidores,
sin nubes. Cada equipo que se acerca se convierte en un nodo de la conversación
y los mensajes saltan de nodo en nodo hasta llegar.

```
 ┌ Canales ─────────┐ ┌ #general ───────────────────────────────┐
 │▸ #general        │ │ [04:12] akaomi: ¿me ves?                │
 │  #charla         │ │ [04:12] ♥ 6a3f: sí, malla viva           │
 │                  │ │                                          │
 ├ Pares            │ │                                          │
 │  ♥ 6a3f  ♥ 2c81  │ │                                          │
 └──────────────────┘ └──────────────────────────────────────────┘
 BlueBl · #general  ⌁ mesh ble · cómodo · Dracula · Chat
```

## Características

- **Cero internet.** Sólo radio Bluetooth LE: anuncio, escaneo y mensajes
  saltando en malla (TTL + deduplicación, retransmisión entre vecinos).
- **Malla real.** Cada nodo reenvía los paquetes que no son suyos mientras
  le quede TTL, así dos personas sin línea directa se hablan a través de un
  tercero.
- **Mensajes firmados.** Identidad Ed25519 + X25519 por equipo; el peer ID
  se deriva de la clave pública y un anuncio que no cuadra se descarta.
- **Compatibilidad de protocolo.** Cabecera, flags, padding determinista,
  compresión raw-deflate, TLV de anuncio y fragmentación de paquetes grandes
  con el mismo formato por el aire que la app de referencia (hay tests que
  comparan byte a byte con vectores fijos).
- **Un solo canal: `#general`.** El chat público viaja como texto UTF-8
  plano, igual que lo hace la app: lo que escribes en BlueBl se ve en su
  pantalla y lo que escribe ella se ve aquí, sin ruido ni mensajes
  cruzados de canales ajenos.
- **42 temas** en cuatro categorías (base y accesibilidad, terminales,
  juegos, cine), con selector en caliente (Ctrl+T) y persistencia.
- **Panel adaptable.** Pantalla ancha con la lista de pares a la izquierda;
  en pantallas estrechas (Termux, Windows) un solo panel que cambias con Tab.
  Tres densidades de texto: `compacto`, `comodo`, `grande`.
- **Detección de dependencias** estilo terminal: `--deps` comprueba radio,
  servicio y adaptador, e indica el comando exacto para instalar lo que
  falte en tu distribución (o en Termux).

## Requisitos

- **Linux** con BlueZ (casi todo lo trae ya).
  - Debian/Ubuntu: `sudo apt install bluez libdbus-1-dev`
  - Fedora: `sudo dnf install bluez dbus-devel`
  - Arch: `sudo pacman -S bluez bluez-utils dbus`
  - Termux: `pkg install bluez`
- Un adaptador Bluetooth LE (cualquier portátil de los últimos años).
- Rust (sólo para compilar desde el código).

Comprueba tu equipo con:

```sh
bluebl --deps
```

## Compilar e instalar

```sh
cargo build --release
cargo install --path .        # deja el binario en ~/.cargo/bin
```

## Uso

```sh
bluebl                    # arranca con la configuración guardada
bluebl -t dracula         # arranca con un tema y lo guarda
bluebl --themes           # lista los 42 temas por categoría
bluebl --deps             # estado de la radio y cómo instalarla
```

Dentro del programa:

| Tecla | Acción |
|---|---|
| `intro` | enviar mensaje |
| `tab` | cambiar de panel |
| `↑ ↓` / `avpag` `rePag` | desplazar el chat |
| `f1` o `ctrl+H` | ayuda (`/help`) |
| `ctrl+W` | quién está en la malla (`/who`) |
| `ctrl+F` | tu huella digital (`/fp`) |
| `ctrl+P` | QR de emparejamiento (`/pair`) |
| `ctrl+N` | tu apodo (`/nick`) |
| `ctrl+L` | limpiar el chat (`/clear`) |
| `ctrl+T` | selector de temas |
| `ctrl+Q` | salir (apaga la radio y guarda la config) |

Comandos de chat:

```
/nick NOMBRE     cambiar apodo          /who            quién está en la malla
/me ACCIÓN       acción en tercera      /peers          lista de pares
/theme NOMBRE    cambiar tema           /density N      compacto | comodo | grande
/pair            QR + huella            /fp [apodo]     huella para comparar
/accept APODO    contacto verificado    /deps           radio del sistema
/clear           vaciar el chat         /help           esta ayuda
```

## Configuración

`~/.config/bluebl/config`, formato `clave=valor`:

```
nick=akaomi
theme=pip-boy-verde
density=comodo
```

## Estado del proyecto

| Fase | Contenido | Estado |
|---|---|---|
| 0 | Protocolo, identidad, temas, TUI, CLI | ✅ |
| 1 | Radio BLE en Linux (BlueZ) | ✅ |
| 2 | Termux y Windows | ⏳ |
| 3 | Couriers, outbox, gossip, favoritos, panic wipe | ⏳ |

La Fase 1 implementa el rol de peripheral (anuncio LE + servidor GATT) y el
rol central (escaneo, conexión y escritura) sobre D-Bus, con retransmisión
en malla y reensamblado de fragmentos.

## Licencia

MIT
