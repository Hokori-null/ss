use ipnetwork::Ipv4Network;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::str::FromStr;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::Duration;
use std::env; // 新增：用于处理命令行参数

// --- 配置参数 ---
// const TARGET_CIDR: &str = "192.168.0.0/24"; // 您想要扫描的网段 - 将从命令行获取
// const TARGET_PORT: u16 = 443; // 您想要检查的端口 - 将从命令行获取
const NUM_THREADS: usize = 50; // 并发线程数
const TIMEOUT_MS: u64 = 500; // 连接超时时间（毫秒）
// --- 配置参数结束 ---

fn scan_ip(ip: Ipv4Addr, port: u16, timeout: Duration) -> Option<Ipv4Addr> {
    let socket_addr = SocketAddr::new(IpAddr::V4(ip), port);
    match TcpStream::connect_timeout(&socket_addr, timeout) {
        Ok(_) => {
            // println!("设备在线: {} (端口 {})", ip, port); // 如果需要实时输出可以取消注释
            Some(ip)
        }
        Err(_) => None,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("用法: {} <网段CIDR> <端口号>", args[0]);
        eprintln!("例如: {} 192.168.1.0/24 80", args[0]);
        return;
    }

    let target_cidr = &args[1];
    let target_port: u16 = match args[2].parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("端口号 '{}' 无效，请输入一个有效的数字端口。", args[2]);
            return;
        }
    };

    println!(
        "主人，开始扫描网段 {} 的端口 {}，使用 {} 个线程，超时时间 {}毫秒...",
        target_cidr, target_port, NUM_THREADS, TIMEOUT_MS
    );

    let network: Ipv4Network = match Ipv4Network::from_str(target_cidr) {
        Ok(net) => net,
        Err(e) => {
            eprintln!("哎呀，网段 '{}' 格式好像不对哦: {}", target_cidr, e);
            return;
        }
    };

    let (tx, rx) = channel::<Ipv4Addr>();
    let timeout = Duration::from_millis(TIMEOUT_MS);

    let mut threads = vec![];
    let mut ips_to_scan = vec![];

    // 收集所有需要扫描的 IP 地址 (排除网络地址和广播地址)
    for ip in network.iter().skip(1) { // network.iter() 包含网络地址和广播地址
        if ip == network.broadcast() {
            continue;
        }
        ips_to_scan.push(ip);
    }
    
    if ips_to_scan.is_empty() && network.prefix() < 31 { // 对于 /31 和 /32 特殊处理
        println!("这个网段太小啦，没有可用的主机 IP 地址可以扫描哦。");
        return;
    } else if network.prefix() == 31 { // /31 网络，有两个可用IP
        ips_to_scan.clear(); // 清空之前可能错误添加的
        ips_to_scan.push(network.network());
        ips_to_scan.push(network.broadcast()); // 在/31中，这两个都是可用地址
    } else if network.prefix() == 32 { // /32 网络，只有一个IP
        ips_to_scan.clear();
        ips_to_scan.push(network.network());
    }


    let total_ips = ips_to_scan.len();
    if total_ips == 0 {
        println!("这个网段 {} 没有可扫描的主机 IP 哦。", target_cidr);
        return;
    }
    println!("总共需要扫描 {} 个 IP 地址...", total_ips);


    let mut ip_idx = 0;
    for _ in 0..NUM_THREADS {
        if ip_idx >= total_ips {
            break; // IP 分配完毕
        }

        // 创建一个IP列表给当前线程
        let mut thread_ips = Vec::new();
        // 简单地将IP分配给线程，可以优化为更均匀的分配方式
        // 这里每个线程启动时就拿走一批IP
        while ip_idx < total_ips {
            thread_ips.push(ips_to_scan[ip_idx]);
            ip_idx += 1;
            if thread_ips.len() >= (total_ips / NUM_THREADS) + 1 && ip_idx < total_ips { // 尽量均匀分配
                 if ip_idx % NUM_THREADS == 0 { // 让一个线程不要拿太多
                    break;
                 }
            }
        }
        
        if thread_ips.is_empty() {
            continue;
        }

        let tx_clone = Sender::clone(&tx);
        let handle = thread::spawn(move || {
            for ip in thread_ips {
                if let Some(online_ip) = scan_ip(ip, target_port, timeout) {
                    if tx_clone.send(online_ip).is_err() {
                        // 如果发送失败，说明主线程可能已经退出了
                        // eprintln!("哎呀，发送结果给主线程失败了，可能是主线程不见了QAQ");
                        return;
                    }
                }
            }
        });
        threads.push(handle);
    }

    // 关闭发送端，这样接收端在所有发送者都结束后会停止阻塞
    drop(tx);

    let mut online_ips = vec![];
    for received_ip in rx {
        online_ips.push(received_ip);
    }

    // 等待所有线程完成
    for handle in threads {
        if handle.join().is_err() {
            eprintln!("呜呜，有一个扫描线程出错了...");
        }
    }

    println!("\n扫描完成！ଘ(੭ˊᵕˋ)੭* ੈ✩‧₊˚");
    if online_ips.is_empty() {
        println!("在网段 {} 上没有发现端口 {} 打开的设备呢。", target_cidr, target_port);
    } else {
        println!("在网段 {} 上发现以下设备在线 (端口 {} 打开):", target_cidr, target_port);
        online_ips.sort(); // 排序一下结果
        for ip in online_ips {
            println!("{}", ip);
        }
    }
}
