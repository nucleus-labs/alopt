use std::sync::Arc;
use std::thread;

use alopt::alo::thread::{Wom, Worl};

#[test]
fn test_worl_sync() {
    let worl = Worl::<i8>::new(0);

    assert_eq!(*worl.read().unwrap(), 0);

    *worl.write().unwrap() += 3;
    assert_eq!(*worl.read().unwrap(), 3);

    worl.clear().unwrap();
    assert!(worl.read().is_err());

    worl.set(67).unwrap();
    assert_eq!(*worl.read().unwrap(), 67);

    {
        let worl_guard = worl.read().unwrap();
        assert!(worl.write().is_err());
        assert_eq!(*worl_guard, 67);
    }

    {
        let mut worl_guard = worl.write().unwrap();
        assert!(worl.write().is_err());
        *worl_guard = 54;
    }
    assert_eq!(*worl.read().unwrap(), 54);
}

#[test]
fn test_worl_threaded() {
    use std::time::Duration;

    let worl = Arc::new(Worl::<i8>::new(0));

    let mut reader_handles = Vec::new();
    for _ in 0..5000 {
        let worl_reader = worl.clone();
        reader_handles.push(thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            match worl_reader.read() {
                Ok(guard) => *guard,
                Err(_) => -1,
            }
        }));
    }

    let worl_writer = worl.clone();
    let writer_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        let mut guard = worl_writer.write().unwrap();
        *guard += 10;
    });
    writer_handle.join().unwrap();

    for handle in reader_handles {
        let value = handle.join().unwrap();
        assert!(value == 0 || value == 10);
    }

    let worl_clear = worl.clone();
    let clear_handle = thread::spawn(move || {
        worl_clear.clear().unwrap();
    });
    clear_handle.join().unwrap();

    assert!(worl.read().is_err());

    let worl_set = worl.clone();
    let set_handle = thread::spawn(move || {
        worl_set.set(42).unwrap();
    });
    set_handle.join().unwrap();

    let value = *worl.read().unwrap();
    assert_eq!(value, 42);
}

#[test]
fn test_wom_sync() {
    let wom = Wom::new(10);

    {
        let v = wom.get_mut().unwrap();
        assert_eq!(*v, 10);
    }

    {
        let mut guard = wom.lock().unwrap();
        *guard += 5;
    }
    assert_eq!(*wom.get_mut().unwrap(), 15);

    let empty_wom = Wom::<i32>::empty();
    assert!(empty_wom.get_mut().is_err());
    assert!(empty_wom.lock().is_err());
}

#[test]
fn test_wom_threaded() {
    let wom = Arc::new(Wom::new(0));
    const THREAD_COUNT: usize = 10;
    let mut handles = Vec::with_capacity(THREAD_COUNT);

    for _ in 0..THREAD_COUNT {
        let wom_clone = Arc::clone(&wom);
        let handle = thread::spawn(move || {
            let mut guard = wom_clone.lock().unwrap();
            *guard += 1;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(*wom.get_mut().unwrap(), THREAD_COUNT as i32);
}
