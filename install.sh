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
#
# Comprueba además que BlueZ no sea viejo (hace falta 5.78 o más) y, si
# lo está, intenta actualizarlo por la vía de tu distro.

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
        sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    else
        printf '%s\n' \
            "install.sh — instala BlueBl y todo lo que necesita para funcionar." \
            "" \
            "  sh install.sh            instala dependencias, compila e instala" \
            "  sh install.sh -y         sin preguntas" \
            "  sh install.sh --no-deps  no toca el sistema: sólo compila" \
            "  sh install.sh --help     esta ayuda" \
            "" \
            "También comprueba BlueZ (>= 5.78) y lo actualiza si hace falta."
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

# ---------------------------------------------------------------- BlueZ
# Con BlueZ anterior a la 5.78 el bug de publicidad extendida rechaza el
# anuncio LE ("Invalid Parameters (0x0d)") y BlueBl se queda en "sin radio".

# Versión de BlueZ ("5.72") o vacío si no se puede leer.
bluez_version() {
    v=""
    if have bluetoothd; then
        v=$(bluetoothd -v 2>/dev/null | head -n1)
    elif [ -x /usr/libexec/bluetooth/bluetoothd ]; then
        v=$(/usr/libexec/bluetooth/bluetoothd -v 2>/dev/null | head -n1)
    elif [ -x /usr/sbin/bluetoothd ]; then
        v=$(/usr/sbin/bluetoothd -v 2>/dev/null | head -n1)
    fi
    # Sin el daemon a mano, el paquete instalado también sirve.
    if [ -z "$v" ] && have dpkg-query; then
        v=$(dpkg-query -W -f='${Version}' bluez 2>/dev/null || true)
    fi
    if [ -z "$v" ] && have rpm; then
        v=$(rpm -q --qf '%{VERSION}' bluez 2>/dev/null || true)
    fi
    # Sólo "mayor.menor" (quita epoch tipo "1:5.72-0ubuntu7").
    printf '%s\n' "$v" | sed -n \
        -e 's/^[0-9][0-9]*://' \
        -e 's/^[^0-9]*\([0-9][0-9]*\)\.\([0-9][0-9]*\).*/\1.\2/p' |
        head -n1
}

# 0 si la versión (mayor.menor) es anterior a 5.78.
bluez_old() {
    major=${1%%.*}
    minor=${1#*.}
    minor=${minor%%.*}
    case "$major$minor" in
        ""|*[!0-9]*) return 1 ;; # sin versión clara: no insistimos
    esac
    if [ "$major" -lt 5 ]; then return 0; fi
    if [ "$major" -eq 5 ] && [ "$minor" -lt 78 ]; then return 0; fi
    return 1
}

# Codename de la base Ubuntu (noble en Mint 22.x / Ubuntu 24.04).
os_release_codename() {
    [ -r /etc/os-release ] || return 0
    # shellcheck disable=SC1091
    (. /etc/os-release; printf '%s' "${UBUNTU_CODENAME:-${VERSION_CODENAME:-}}")
}

# Nombre bonito de la distro, para los mensajes.
distro_nombre() {
    [ -r /etc/os-release ] || { printf 'desconocida'; return 0; }
    # shellcheck disable=SC1091
    (. /etc/os-release; printf '%s' "${PRETTY_NAME:-$ID}")
}

# Actualización por el gestor propio de la distro.
actualizar_bluez_nativo() {
    case "$1" in
        debian) as_root apt-get update -qq && as_root apt-get install -y bluez ;;
        fedora) as_root dnf upgrade -y bluez ;;
        arch)   as_root pacman -Syu --noconfirm bluez ;;
        suse)   as_root zypper refresh && as_root zypper install -y bluez ;;
        termux) pkg install -y bluez ;;
        *)      return 1 ;;
    esac
}

# Respaldo de la configuración de BlueZ antes de tocar el paquete.
respaldo_main_conf() {
    if [ -f /etc/bluetooth/main.conf ] && [ ! -f /etc/bluetooth/main.conf.bluebl-bak ]; then
        as_root cp /etc/bluetooth/main.conf /etc/bluetooth/main.conf.bluebl-bak || true
    fi
}

# Ruta en noble: PPA de backports de BlueZ (la que resolvió el caso real).
actualizar_bluez_ppa() {
    respaldo_main_conf
    if ! have add-apt-repository; then
        as_root apt-get install -y software-properties-common || return 1
    fi
    as_root add-apt-repository -y ppa:giner/bluez || return 1
    as_root apt-get update -qq || return 1
    as_root apt-get install -y bluez
}

# Plan B: descarga los .deb de la fuente real del PPA en Launchpad
# (pkgs.org sólo lista; la descarga va al pool del PPA) y los instala.
descargar_debs_bluez() {
    arch=$1
    pool="https://ppa.launchpadcontent.net/giner/bluez/ubuntu/pool/main/b/bluez"
    if have curl; then
        idx=$(curl -fsSL "$pool/") || return 1
    elif have wget; then
        idx=$(wget -qO- "$pool/") || return 1
    else
        return 1
    fi
    # Última versión de bluez y su libbluetooth3 para nuestra arquitectura.
    deb=$(printf '%s\n' "$idx" | grep -o "bluez_[^\"]*_${arch}\\.deb" | sort -uV | tail -n1)
    [ -n "$deb" ] || return 1
    ver=${deb#bluez_}
    ver=${ver%_"$arch".deb}
    lib="libbluetooth3_${ver}_${arch}.deb"
    printf '%s\n' "$idx" | grep -q -F "$lib" || return 1
    tmp=$(mktemp -d) || return 1
    if have curl; then
        curl -fsSL -o "$tmp/$deb" "$pool/$deb" && curl -fsSL -o "$tmp/$lib" "$pool/$lib" ||
            { rm -rf "$tmp"; return 1; }
    else
        wget -q -O "$tmp/$deb" "$pool/$deb" && wget -q -O "$tmp/$lib" "$pool/$lib" ||
            { rm -rf "$tmp"; return 1; }
    fi
    st=0
    (cd "$tmp" && as_root apt-get install -y "./$deb" "./$lib") || st=$?
    rm -rf "$tmp"
    return "$st"
}

# Arquitectura de los .deb del PPA (amd64 / i386); vacío si no aplica.
deb_arch() {
    case "$(uname -m)" in
        x86_64) echo amd64 ;;
        i?86)   echo i386 ;;
        *)      echo "" ;;
    esac
}

# Reinicia el servicio y espera a que levante.
reiniciar_bluetooth() {
    as_root systemctl restart bluetooth || return 1
    i=0
    while [ "$i" -lt 10 ]; do
        if systemctl is-active --quiet bluetooth 2>/dev/null; then return 0; fi
        sleep 1
        i=$((i + 1))
    done
    return 1
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

# 2b. BlueZ viejo: sin 5.78 el bug de publicidad extendida rechaza el
# anuncio LE ("Invalid Parameters (0x0d)") y BlueBl se queda en "sin radio".
if [ "$WITH_DEPS" -eq 1 ]; then
    bz=$(bluez_version)
    if bluez_old "$bz"; then
        warn "BlueZ $bz es viejo (hace falta 5.78 o más): el anuncio LE puede ser rechazado"
        if ask "¿Intento actualizar BlueZ ahora?"; then
            familia=$(platforma)
            codename=$(os_release_codename)
            hecho=0

            # a) la vía propia de la distro
            if actualizar_bluez_nativo "$familia"; then
                nuevo=$(bluez_version)
                if [ -n "$nuevo" ] && ! bluez_old "$nuevo"; then
                    hecho=1
                    ok "BlueZ $nuevo (gestor de la distro)"
                fi
            fi

            # b) en bases noble el repo oficial se queda en 5.72: PPA
            if [ "$hecho" -eq 0 ] && [ "$familia" = debian ] && [ "$codename" = noble ]; then
                info "tu base es noble: pruebo el PPA de backports de BlueZ…"
                if actualizar_bluez_ppa; then
                    nuevo=$(bluez_version)
                    if [ -n "$nuevo" ] && ! bluez_old "$nuevo"; then
                        hecho=1
                        ok "BlueZ $nuevo (PPA)"
                    fi
                fi
                # c) plan B: los .deb directos del pool del PPA en Launchpad
                if [ "$hecho" -eq 0 ]; then
                    arch=$(deb_arch)
                    if [ -n "$arch" ] && descargar_debs_bluez "$arch"; then
                        nuevo=$(bluez_version)
                        if [ -n "$nuevo" ] && ! bluez_old "$nuevo"; then
                            hecho=1
                            ok "BlueZ $nuevo (plan B: .deb del PPA)"
                        fi
                    fi
                fi
            fi

            if [ "$hecho" -eq 1 ]; then
                if have systemctl; then
                    reiniciar_bluetooth ||
                        warn "reinicia el servicio a mano: sudo systemctl restart bluetooth"
                fi
            else
                warn "no he podido actualizarlo automáticamente; hazlo a mano con el gestor de tu distro"
                warn "guía: https://pkgs.org/search/?q=bluez"
            fi
        else
            warn "BlueZ se queda en $bz: sin 5.78 el anuncio LE puede ser rechazado"
            warn "guía para actualizar: https://pkgs.org/search/?q=bluez"
        fi
    fi
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
