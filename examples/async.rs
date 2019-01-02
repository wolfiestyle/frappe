use frappe::Sink;
use rand::Rng;

use std::sync::{mpsc, Arc, RwLock, Mutex};
use std::thread::{self, Thread};
use std::time::Duration;

// this represents some external library that provides an event loop
#[derive(Clone)]
struct Updater
{
    jobs: Arc<RwLock<Vec<Box<dyn Fn() -> bool + Send + Sync>>>>,
    main_thread: Thread,
}

impl Updater
{
    fn new() -> Self
    {
        Updater{
            jobs: Default::default(),
            main_thread: thread::current(),
        }
    }

    // adds a new job. it will run periodically until it returns true
    fn register<F>(&self, f: F)
        where F: Fn() -> bool + Send + Sync + 'static
    {
        self.jobs.write().unwrap().push(Box::new(f));
    }

    // runs all current jobs
    fn update(&self)
    {
        self.jobs.write().unwrap().retain(|f| !f());
    }

    // waits until data is available
    fn wait(&self)
    {
        thread::park();  // can't select channels (yet), so we manually signal it
    }

    // need this to call .unpark() on it
    fn get_main_thread(&self) -> Thread
    {
        self.main_thread.clone()
    }

    // checks if there are pending jobs
    fn pending(&self) -> bool
    {
        !self.jobs.read().unwrap().is_empty()
    }
}

fn main()
{
    let updater = Updater::new();
    let upd = updater.clone();

    let sink = Sink::new();

    let result = sink.stream()
        // we'll do this part on another thread
        .map_n(move |arg, sink_| {
            let n = *arg;
            let (tx, rx) = mpsc::channel();
            let main_th = upd.get_main_thread();
            // our expensive computation (sleep sort)
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(n));
                tx.send(n).unwrap();
                main_th.unpark(); // signal that there data is available
            });
            // store the sink and use it later to return the value when it's ready
            let rx_ = Mutex::new(rx);
            upd.register(move || rx_.lock().unwrap().try_recv().map(|v| sink_.send(v)).is_ok());
        })
        // the rest of the chain continues on the main thread as normal
        .collect::<Vec<_>>();

    // now send some random values
    let mut rng = rand::thread_rng();
    sink.feed((0..10).map(|_| rng.gen_range(0, 100)));

    // the main loop dispatches the results back to the stream
    while updater.pending()
    {
        updater.wait();
        updater.update();
        println!("{:?}", result.sample());
    }
}
