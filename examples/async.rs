use frappe::Sink;
use rand::Rng;
use std::thread;
use std::time::Duration;

fn main() {
    let sink = Sink::new();

    let result = sink
        .stream()
        // we'll do this part on another thread
        .map_n(move |arg, sender| {
            let n = *arg;
            let main_th = thread::current();
            // our expensive computation (sleep sort)
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(n));
                sender.send(n);
                main_th.unpark(); // signal that there is data available
            });
        })
        // the rest of the chain continues on the main thread as normal
        .collect::<Vec<_>>();

    // now send some random values
    let mut rng = rand::thread_rng();
    sink.feed((0..10).map(|_| rng.gen_range(0, 100)));

    // receive the results on the main thread
    loop {
        thread::park();
        let res = result.sample();
        println!("{:?}", res);
        if res.len() >= 10 {
            break;
        }
    }
}
