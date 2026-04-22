use sysinfo::System; // On n'importe plus les traits Ext
use std::fmt;
use chrono::Local;
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::thread;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// --- ÉTAPE 1 : MODÉLISATION DES DONNÉES ---
#[derive(Debug, Clone)]
struct SystemSnapshot {
    cpu_usage: f32,
    mem_total: u64,
    mem_used: u64,
    processes: Vec<ProcessInfo>,
    timestamp: String,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu: f32,
}

impl fmt::Display for SystemSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "\n--- SYSWATCH SNAPSHOT [{}] ---", self.timestamp)?;
        writeln!(f, "CPU Global: {:.2}%", self.cpu_usage)?;
        writeln!(f, "RAM: {} / {} MB", self.mem_used / 1024 / 1024, self.mem_total / 1024 / 1024)?;
        writeln!(f, "Top 5 Processus (CPU):")?;
        for p in &self.processes {
            writeln!(f, "  - PID {}: {} ({:.2}%)", p.pid, p.name, p.cpu)?;
        }
        Ok(())
    }
}

// --- ÉTAPE 2 : COLLECTE RÉELLE ---
fn collect_snapshot(sys: &mut System) -> SystemSnapshot {
    // En v0.30, refresh_all() est une méthode directe de System
    sys.refresh_all();
    
    let mut procs: Vec<ProcessInfo> = sys.processes()
        .values()
        .map(|p| ProcessInfo {
            pid: p.pid().as_u32(),
            name: p.name().to_string(),
            cpu: p.cpu_usage(),
        })
        .collect();
    
    procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    procs.truncate(5);

    SystemSnapshot {
        // Accès direct aux méthodes sans traits Ext
        cpu_usage: sys.global_cpu_info().cpu_usage(),
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        processes: procs,
        timestamp: Local::now().format("%H:%M:%S").to_string(),
    }
}

// --- ÉTAPE 3 : FORMATAGE DES RÉPONSES ---
fn format_response(snapshot: &SystemSnapshot, command: &str) -> String {
    match command.trim().to_lowercase().as_str() {
        "cpu" => format!("Usage CPU: {:.2}%\n", snapshot.cpu_usage),
        "mem" => format!("RAM: {}/{} MB\n", snapshot.mem_used/1024/1024, snapshot.mem_total/1024/1024),
        "ps" => {
            let mut res = String::from("Top 5 Processus:\n");
            for p in &snapshot.processes {
                res.push_str(&format!("PID {}: {} ({:.2}%)\n", p.pid, p.name, p.cpu));
            }
            res
        },
        "all" => format!("{}", snapshot),
        "help" => "Commandes disponibles : cpu, mem, ps, all, help, quit\n".to_string(),
        _ => "Commande inconnue. Tapez 'help' pour l'aide.\n".to_string(),
    }
}

// --- ÉTAPE 4 : SERVEUR TCP MULTI-THREADÉ ---
fn handle_client(mut stream: TcpStream, shared_data: Arc<Mutex<SystemSnapshot>>) {
    let mut buffer = [0; 512];
    println!("Nouveau client connecté.");

    while match stream.read(&mut buffer) {
        Ok(size) if size > 0 => {
            let command = String::from_utf8_lossy(&buffer[..size]);
            let cmd_clean = command.trim();
            
            if cmd_clean == "quit" {
                let _ = stream.write_all(b"Au revoir!\n");
                false 
            } else {
                let snapshot = shared_data.lock().unwrap();
                let response = format_response(&snapshot, cmd_clean);
                let _ = stream.write_all(response.as_bytes());
                true
            }
        },
        _ => {
            println!("Client déconnecté.");
            false
        },
    } {}
}

fn main() {
    let mut sys = System::new_all();
    let initial_snapshot = collect_snapshot(&mut sys);
    let shared_data = Arc::new(Mutex::new(initial_snapshot));

    let data_for_update = Arc::clone(&shared_data);
    thread::spawn(move || {
        let mut s = System::new_all();
        loop {
            thread::sleep(Duration::from_secs(5));
            let new_snap = collect_snapshot(&mut s);
            
            // On clone le timestamp AVANT le transfert de propriété (move)
            let ts = new_snap.timestamp.clone(); 
            
            let mut data = data_for_update.lock().unwrap();
            *data = new_snap; 
            
            println!("Métriques système rafraîchies à {}", ts);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:7878").expect("Impossible de lier le port 7878");
    println!("Serveur SysWatch opérationnel sur 127.0.0.1:7878");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let data = Arc::clone(&shared_data);
                thread::spawn(move || handle_client(s, data));
            }
            Err(e) => println!("Erreur de connexion : {}", e),
        }
    }
}