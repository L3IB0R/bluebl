#!/bin/sh
# install.sh — instala BlueBl y todo lo que necesita para funcionar.
#
#   sh install.sh            instala dependencias, compila y deja bluebl listo
#   sh install.sh -y         sin preguntas (instala lo que haga falta)
#   sh install.sh --no-deps  no toca el sistema: sólo compila e instala
#   sh install.sh --help     esta ayuda
#
# Sólo pide sudo para instalar paquetes del sistema; el resto (compilar,
# copiar el binario) va en tu directorio de usuario.

set -eu

REPO_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_DIR="$HOME/.local/bin"
ASSUME_YES=0
WITH_DEPS=1
# Rellénalo si algún día el código vive en otro sitio (se usa sólo cuando
# ejecutas el script sin tener el código a mano).
REPO_URL="https://github.com/L3IB0R/bluebl.git"

info() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m ok\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!! \033[0m %s\n' "$*"; }
err()  { printf '\033[1;31m error\033[0m %s\n' "$*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
    # Con el script en tubería (curl | sh) no hay archivo que leer.
    if [ -f "$0" ]; then
        sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
    else
        printf '%s\n' \
            "install.sh — instala BlueBl y todo lo que necesita para funcionar." \
            "" \
            "  sh install.sh            instala dependencias, compila e instala" \
            "  sh install.sh -y         sin preguntas" \
            "  sh install.sh --no-deps  no toca el sistema: sólo compila" \
            "  sh install.sh --help     esta ayuda"
    fi
    exit 0
}

for arg in "$@"; do
    case "$arg" in
        -h|--help)  usage ;;
        -y|--yes)   ASSUME_YES=1 ;;
        --no-deps)  WITH_DEPS=0 ;;
        *) err "opción desconocida: $arg (prueba con --help)"; exit 2 ;;
    esac
done

ask() {
    # ask "pregunta" → devuelve 0 si la respuesta es sí
    [ "$ASSUME_YES" -eq 1 ] && return 0
    printf '%s [s/N] ' "$1"
    read -r respuesta || return 1
    case "$respuesta" in
        s|S|si|sí|y|Y|yes) return 0 ;;
        *) return 1 ;;
    esac
}

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif have sudo; then
        sudo "$@"
    else
        err "hacen falta permisos de administrador para: $*"
        err "ejecútalo tú a mano: $*"
        return 1
    fi
}

# ---------------------------------------------------------------- plataforma

is_termux() {
    [ -n "${TERMUX_VERSION:-}" ] || {
        [ -n "${PREFIX:-}" ] && [ -d /data/data/com.termux ]
    }
}

platforma() {
    if is_termux; then
        echo termux
        return
    fi
    case "$(uname -s)" in
        Linux)
            if [ -r /etc/os-release ]; then
                # shellcheck disable=SC1091
                . /etc/os-release
                familia="$(echo "${ID:-} ${ID_LIKE:-}" | tr '[:upper:]' '[:lower:]')"
                case "$familia" in
                    *debian*|*ubuntu*|*anduinos*) echo debian ;;
                    *fedora*|*rhel*|*centos*)     echo fedora ;;
                    *arch*)                       echo arch ;;
                    *suse*)                       echo suse ;;
                    *)                            echo otro ;;
                esac
            else
                echo otro
            fi
            ;;
        *) echo otro ;;
    esac
}

# ------------------------------------------------------------- dependencias

paquetes_para() {
    case "$1" in
        debian) echo "bluez libdbus-1-dev pkg-config build-essential" ;;
        fedora) echo "bluez dbus-devel pkgconf-pkg-config gcc make" ;;
        arch)   echo "bluez dbus pkgconf gcc make" ;;
        suse)   echo "bluez-devel dbus-1-devel pkg-config gcc make" ;;
        termux) echo "bluez rust pkg-config" ;;
        *)      echo "" ;;
    esac
}

instalar_comando() {
    case "$1" in
        debian) echo "sudo apt install -y" ;;
        fedora) echo "sudo dnf install -y" ;;
        arch)   echo "sudo pacman -S --needed --noconfirm" ;;
        suse)   echo "sudo zypper install -y" ;;
        termux) echo "pkg install -y" ;;
        *)      echo "" ;;
    esac
}

faltan_sistema() {
    # Herramientas que sólo se comprueban fuera de Termux: ahí no hay sudo.
    if is_termux; then
        have pkg-config || echo "pkg-config"
        return
    fi
    have cc || echo "compilador C (build-essential / gcc)"
    have pkg-config || echo "pkg-config"
    if ! pkg-config --exists dbus-1 2>/dev/null; then
        echo "cabeceras de D-Bus (libdbus-1-dev / dbus-devel)"
    fi
    if ! have bluetoothctl && ! [ -d /var/lib/bluetooth ] && ! have hciconfig; then
        echo "BlueZ (bluez)"
    fi
}

# ------------------------------------------------------------------ pasos

info "BlueBl — instalador"

# 1. Fuente del código
if [ -f "$REPO_DIR/Cargo.toml" ]; then
    src="$REPO_DIR"
    ok "código en $src"
elif [ -n "$REPO_URL" ] && have git; then
    src="$HOME/.local/share/bluebl"
    info "descargando el código…"
    if [ -d "$src/.git" ]; then
        git -C "$src" pull --ff-only
    else
        git clone "$REPO_URL" "$src"
    fi
    ok "código en $src"
else
    err "este script necesita el código de BlueBl junto a él."
    err "clona el repositorio y ejecuta install.sh dentro:"
    err "    git clone <URL-del-repositorio> && cd bluebl && sh install.sh"
    exit 1
fi

# 2. Dependencias del sistema
if [ "$WITH_DEPS" -eq 1 ]; then
    faltan=$(faltan_sistema | tr '\n' ' ' | sed 's/ *$//')
    if [ -z "$faltan" ]; then
        ok "el sistema ya tiene lo que hace falta"
    else
        familia=$(platforma)
        inst=$(instalar_comando "$familia")
        paq=$(paquetes_para "$familia")
        if [ -z "$inst" ] || [ -z "$paq" ]; then
            warn "no reconozco tu distribución: instala a mano → $faltan"
            warn "guía de dependencias en README.md (sección Requisitos)"
        else
            warn "faltan en tu sistema: $faltan"
            if ask "¿Los instalo con: $inst $paq ?"; then
                info "instalando paquetes del sistema…"
                # shellcheck disable=SC2086
                as_root $inst $paq
                ok "paquetes instalados"
            else
                warn "los dejo para ti. Cuando los tengas:"
                warn "    $inst $paq"
            fi
        fi
    fi
else
    info "sin tocar el sistema (--no-deps)"
fi

# 3. Rust (sin él no hay nada que compilar)
if ! have cargo; then
    warn "no encuentro Rust (cargo)"
    if have curl && ask "¿Instalo Rust con rustup (sólo en tu usuario)?"; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env" 2>/dev/null || PATH="$HOME/.cargo/bin:$PATH"
        export PATH="$HOME/.cargo/bin:$PATH"
    else
        err "instala Rust con:  curl https://sh.rustup.rs -sSf | sh"
        exit 1
    fi
fi
ok "Rust $(rustc --version 2>/dev/null | awk '{print $2}')"

# 4. Compilar
info "compilando BlueBl (release, unos segundos)…"
(cd "$src" && cargo build --release)
ok "compilado"

# 5. Instalar el binario
mkdir -p "$BIN_DIR"
cp "$src/target/release/bluebl" "$BIN_DIR/bluebl.tmp"
mv "$BIN_DIR/bluebl.tmp" "$BIN_DIR/bluebl"
chmod 755 "$BIN_DIR/bluebl"
ok "instalado en $BIN_DIR/bluebl ($("$BIN_DIR/bluebl" --version))"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        warn "$BIN_DIR no está en tu PATH; añade a tu ~/.bashrc o ~/.zshrc:"
        warn "    export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac

# 6. Estado de la radio
info "comprobando la radio Bluetooth…"
if "$BIN_DIR/bluebl" --deps; then
    ok "radio lista"
else
    warn "la radio aún no está lista. Cosas habituales:"
    warn "    sudo rfkill unblock bluetooth   # radio bloqueada"
    warn "    sudo systemctl enable --now bluetooth   # servicio parado"
    warn "    (en Termux: termux-setup-storage y pkg install bluez)"
fi

printf '\n'
ok "listo — ejecuta:  bluebl"
info "ayuda dentro del programa: F1 o ctrl+H"
info "atajos: ctrl+W quién está · ctrl+F tu huella · ctrl+P QR · ctrl+L limpiar"
