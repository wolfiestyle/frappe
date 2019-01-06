use frappe::Sink;
use rand::distributions::Uniform;
use rand::Rng;
use std::thread;
use std::time::Duration;

fn main() {
    let sink = Sink::new();

    // build the chain on the main thread
    let result = sink
        .stream()
        // we'll do this part on another thread
        .map_n(|arg, sender| {
            let n = *arg;
            // our expensive computation (sleep sort)
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(n));
                sender.send(n); // return the computed value
            });
        })
        // the rest of the stream chain is executed on the thread that called `Sender::send`
        .fold(Vec::new(), |mut vec, n| {
            vec.push(*n);
            vec
        });

    // now send some random values
    let mut rng = rand::thread_rng();
    let input: Vec<_> = rng
        .sample_iter(&Uniform::new_inclusive(0, 100))
        .take(10)
        .collect();
    println!("input:  {:?}", input);
    sink.feed(input);

    // wait a bit for the threads to finish
    thread::sleep(Duration::from_millis(150));

    // receive the results on the main thread
    println!("result: {:?}", result.sample());
}
