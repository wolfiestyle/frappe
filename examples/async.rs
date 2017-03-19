extern crate frappe;
use frappe::{Sink, Signal};

use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc;
use std::thread::{self, Thread};
use std::time::Duration;

#[derive(Clone)]
struct Updater
{
    jobs: Rc<RefCell<Vec<Box<Fn() -> bool>>>>,
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

    fn register<F>(&self, f: F)
        where F: Fn() -> bool + 'static
    {
        self.jobs.borrow_mut().push(Box::new(f));
    }

    fn update(&self)
    {
        self.jobs.borrow_mut().retain(|f| !f());
    }

    fn wait(&self)
    {
        thread::park();
        self.update();
    }

    fn pending(&self) -> bool
    {
        !self.jobs.borrow().is_empty()
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
            let main_th = upd.main_thread.clone();
            // sleep sort
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(n));
                tx.send(n).unwrap();
                main_th.unpark();
            });
            // store the sink and use it later to return the value when it's ready
            upd.register(move || rx.try_recv().map(|v| sink_.send(v)).is_ok());
        })
        // rest of the chain continues on the main thread
        .fold(vec![], |mut vec, n| { vec.push(*n); vec });

    sink.feed(vec![5, 6, 10, 42, 1, 94, 22, 33, 7]);

    // the main loop dispatches results back to the stream
    while updater.pending()
    {
        updater.wait();
        println!("{:?}", result.sample());
    }
}
