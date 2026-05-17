use std::net::IpAddr;

pub struct TunRouting {
    handle: net_route::Handle,
    ifindex: u32,

    #[cfg(target_os = "linux")]
    table: Option<u8>,
}

impl TunRouting {
    pub fn new(ifindex: u32) -> std::io::Result<Self> {
        let handle = net_route::Handle::new()?;
        Ok(Self {
            handle,
            ifindex,

            #[cfg(target_os = "linux")]
            table: None,
        })
    }

    pub fn with_table(mut self, table: u8) -> Self {
        self.table = Some(table);
        self
    }

    fn route(&self, addr: IpAddr, prefix: u8) -> net_route::Route {
        let mut r = net_route::Route::new(addr, prefix).with_ifindex(self.ifindex);

        #[cfg(target_os = "linux")]
        if let Some(table) = self.table {
            r = r.with_table(table)
        }

        r
    }

    pub async fn add(&self, addr: IpAddr, prefix: u8) -> std::io::Result<()> {
        let route = self.route(addr, prefix);
        self.handle.add(&route).await
    }

    pub async fn remove(&self, addr: IpAddr, prefix: u8) -> std::io::Result<()> {
        let route = self.route(addr, prefix);
        self.handle.delete(&route).await
    }
}
