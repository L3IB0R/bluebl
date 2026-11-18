//! Sonda: registra una aplicación GATT mínima en BlueZ y enseña qué ve
//! BlueZ (GetManagedObjects sobre nuestra propia conexión) más el error
//! exacto de RegisterApplication. Herramienta de diagnóstico.

use std::collections::HashMap;
use zbus::interface;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const SERVICE_UUID: &str = "f47b5e2d-4a9e-4c5a-9b3f-8e1d2c3a4b5c";
const CHAR_UUID: &str = "a1b2c3d4-e5f6-4a5b-8c9d-0e1f2a3b4c5d";
const ROOT: &str = "/probe";

struct Svc;

#[interface(name = "org.bluez.GattService1")]
impl Svc {
    #[zbus(property)]
    #[zbus(name = "UUID")]
    fn uuid(&self) -> String {
        SERVICE_UUID.to_string()
    }

    #[zbus(property)]
    fn primary(&self) -> bool {
        true
    }
}

struct Char;

#[interface(name = "org.bluez.GattCharacteristic1")]
impl Char {
    #[zbus(property)]
    #[zbus(name = "UUID")]
    fn uuid(&self) -> String {
        CHAR_UUID.to_string()
    }

    #[zbus(property)]
    fn service(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/probe/service0").expect("path")
    }

    #[zbus(property)]
    fn flags(&self) -> Vec<String> {
        vec![
            "read".into(),
            "write".into(),
            "write-without-response".into(),
            "notify".into(),
        ]
    }

    #[zbus(property)]
    fn value(&self) -> Vec<u8> {
        Vec::new()
    }

    #[zbus(property)]
    fn notifying(&self) -> bool {
        false
    }

    async fn write_value(
        &self,
        _value: Vec<u8>,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn read_value(
        &self,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    async fn start_notify(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn stop_notify(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    // `--session`: se registra en el bus de sesión (política permisiva) para
    // poder leerse a sí mismo y ver los nombres/tipos reales de propiedades.
    let session = std::env::args().any(|a| a == "--session");
    let conn = if session {
        zbus::Connection::session().await.expect("bus de sesión")
    } else {
        zbus::Connection::system().await.expect("bus del sistema")
    };

    conn.object_server()
        .at("/probe/service0", Svc)
        .await
        .expect("servicio");
    conn.object_server()
        .at("/probe/service0/char0", Char)
        .await
        .expect("characteristic");
    conn.object_server()
        .at(ROOT, zbus::fdo::ObjectManager)
        .await
        .expect("object manager");

    // ── Qué servimos nosotros mismos ────────────────────────────────────
    let me = conn
        .unique_name()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "sin-nombre".into());
    println!("conexión propia: {me}");

    let self_om = zbus::Proxy::new(
        &conn,
        me.as_str(),
        ROOT,
        "org.freedesktop.DBus.ObjectManager",
    )
    .await
    .expect("proxy propio");
    match self_om.call_method("GetManagedObjects", &()).await {
        Ok(msg) => {
            let objects: HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> =
                msg.body().deserialize().expect("cuerpo legible");
            println!("\n== GetManagedObjects (lo que BlueZ verá) ==");
            for (path, ifaces) in &objects {
                println!("{path}");
                for (iface, props) in ifaces {
                    println!("  {iface}");
                    for (name, value) in props {
                        println!("    {name} = {value:?}");
                    }
                }
            }
        }
        Err(e) => println!("\n!! GetManagedObjects propio falló: {e}"),
    }

    // ── Registro en BlueZ (sólo en el bus del sistema) ──────────────────
    if session {
        // En el bus de sesión no hay BlueZ: nos limitamos a leernos.
        println!("(bus de sesión: sin BlueZ, sólo introspección)");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        return;
    }

    let adapter = "/org/bluez/hci0";
    let gatt = zbus::Proxy::new(&conn, "org.bluez", adapter, "org.bluez.GattManager1")
        .await
        .expect("GattManager1");

    let root = OwnedObjectPath::try_from(ROOT).expect("path");
    let opts: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    println!("\n== RegisterApplication({ROOT}) ==");
    match gatt.call_method("RegisterApplication", &(root, opts)).await {
        Ok(_) => println!("¡REGISTRADA!"),
        Err(e) => println!("error: {e:?}"),
    }

    tokio::time::sleep(std::time::Duration::from_secs(45)).await;

    let root2 = OwnedObjectPath::try_from(ROOT).expect("path");
    let _ = gatt.call_method("UnregisterApplication", &(root2,)).await;
    println!("desregistrada, adiós");
}
