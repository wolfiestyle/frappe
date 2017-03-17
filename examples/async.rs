extern crate frappe;
use frappe::{Sink, Signal, AsyncTask};

use std::rc::Rc;
use std::cell::RefCell;
use std::sync::{mpsc, Arc};
use std::thread::{self, Thread};
use std::time::Duration;

#[derive(Clone)]
struct ThreadTask<A, B>(Arc<Fn(A) -> B + Send + Sync + 'static>);

impl<A, B> ThreadTask<A, B>
{
    fn new<F>(f: F) -> Self
        where F: Fn(A) -> B + Send + Sync + 'static
    {
        ThreadTask(Arc::new(f))
    }
}

impl<A, B> AsyncTask for ThreadTask<A, B>
    where A: Send + 'static, B: Send + 'static
{
    type Input = A;
    type Output = B;
    type Handler = Updater;

    fn run<F>(&self, handler: &Self::Handler, input: Self::Input, ret: F)
        where F: Fn(Self::Output) -> bool + 'static
    {
        let f = self.0.clone();
        let (tx, rx) = mpsc::channel();
        let main_th = handler.main_thread.clone();
        thread::spawn(move || {
            tx.send(f(input)).unwrap();
            main_th.unpark();
        });
        handler.register(move || rx.try_recv().map(|v| ret(v)).is_ok())
    }
}

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

    let sleep_sort = ThreadTask::new(|n| {
        thread::sleep(Duration::from_millis(n));
        n
    });

    let sink = Sink::new();
    let result = sink.stream()
        .map_async(updater.clone(), sleep_sort)
        .fold(vec![], |mut vec, n| { vec.push(*n); vec });

    sink.feed(vec![5, 6, 10, 42, 1, 94, 22, 33, 7]);

    while updater.pending()
    {
        updater.wait();
        println!("{:?}", result.sample());
    }
}
