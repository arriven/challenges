use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpListener, TcpStream,
};
use tracing::{debug, info, trace};

pub async fn run(app: crate::config::App) -> anyhow::Result<()> {
    info!("running {}", app.name);
    let lb = Arc::new(LoadBalancer::from_targets(app.targets));
    let handles: Vec<_> = app
        .ports
        .into_iter()
        .map(|port| tokio::spawn(listen(lb.clone(), port)))
        .collect();
    futures::future::join_all(handles).await;
    Ok(())
}

// use simple round-robin
struct LoadBalancer {
    targets: Vec<String>,
    // we could use atomic here, but I'm assuming that we usually have
    // middle-high session time and mutex overhead is not significant
    // TODO: validate with metrics
    current: Mutex<usize>,
}

impl LoadBalancer {
    fn from_targets(targets: Vec<String>) -> Self {
        Self {
            targets,
            current: Mutex::new(0),
        }
    }

    fn get_next_id(&self) -> usize {
        let mut current = self.current.lock().unwrap();
        let result = *current;
        *current = (result + 1) % self.targets.len();
        result
    }

    async fn connect(&self) -> anyhow::Result<TcpStream> {
        let start = self.get_next_id();
        // Note: this can lead to uneven distribution of connections if some of the targets are unhealthy
        // Potential solutions would be
        // a) update current index once connection establishes
        //   (could still be uneven, although with lower chance)
        // b) use get_next_id on each iteration with a sane amount of retries
        //   (could lead to failed connection even if there are healthy targets)
        // c) track unhealthy targets with atomic markers and check for them in get_next_id
        //   (would require a background task to check if targets are back online, potential delays)
        // d) using a different loadbalancing algorithm
        //   (would still need to ensure it handles unhealthy targets correctly)
        for i in 0..self.targets.len() {
            let index = (start + i) % self.targets.len();
            let target = &self.targets[index];
            debug!("connecting to {}", target);
            let stream = match TcpStream::connect(target).await {
                Ok(stream) => stream,
                Err(_) => continue,
            };
            debug!("connected to {}", target);
            return Ok(stream);
        }
        Err(anyhow::anyhow!("failed to connect to any of the backends"))
    }
}

// TODO: we don't really do anything with the error right now
// consider tracing it in place or sending somewhere via channel
// same for handle() and forward()
async fn listen(lb: Arc<LoadBalancer>, port: u16) -> anyhow::Result<()> {
    // TODO: pass address via commandline/env to be able to select specific interface?
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    info!("listening on port {}", port);
    loop {
        let (socket, _) = listener.accept().await?;
        debug!("accepted connection on port {}", port);
        let lb = lb.clone();
        tokio::spawn(handle(lb, socket));
    }
}

async fn handle(lb: Arc<LoadBalancer>, client: TcpStream) -> anyhow::Result<()> {
    let backend = lb.connect().await?;
    let (client_read, client_write) = client.into_split();
    let (backend_read, backend_write) = backend.into_split();
    tokio::spawn(forward(client_read, backend_write));
    tokio::spawn(forward(backend_read, client_write));
    Ok(())
}

// consider using buffered readers/writers
// pros: performance
// cons: what if clients need high responsiveness?
async fn forward(mut read: OwnedReadHalf, mut write: OwnedWriteHalf) -> anyhow::Result<()> {
    let mut buf = vec![0; 1024];

    loop {
        let n = read.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        trace!("received {} bytes", n);
        write.write_all(&buf[0..n]).await?;
    }
}
