use std::sync::Arc;
use std::thread;

use alopt::alo::thread::{Wom, Worl};

#[test]
fn test_worl_sync() {
    let worl = Worl::<i8>::new(0).set_timeout(500);

    assert_eq!(*worl.read().unwrap(), 0);

    *worl.write().unwrap() += 3;
    assert_eq!(*worl.read().unwrap(), 3);

    worl.clear().unwrap();
    assert!(worl.read().is_err());

    assert_eq!(*worl.set(67).unwrap(), 67);
    assert_eq!(*worl.read().unwrap(), 67);

    worl.write().unwrap().clear();
    assert!(worl.read().is_err());

    worl.set(-1).unwrap();
    assert_eq!(*worl.read().unwrap(), -1);
    worl.write().unwrap().set(-7);
    assert_eq!(*worl.read().unwrap(), -7);

    {
        let worl_guard = worl.read().unwrap();
        worl.read().unwrap();
        assert_eq!(*worl_guard, -7);
    }

    {
        let worl_guard = worl.read().unwrap();
        assert!(worl.write().is_err());
        assert_eq!(*worl_guard, -7);
    }

    {
        let mut worl_guard = worl.write().unwrap();
        assert!(worl.read().is_err());
        assert_eq!(*worl_guard, -7);
        *worl_guard = 42;
    }
    assert_eq!(*worl.read().unwrap(), 42);

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

    let worl = Arc::new(Worl::<i8>::new(0).set_timeout(u16::MAX));

    let mut reader_handles = Vec::new();
    for _ in 0..5000 {
        let worl_reader = worl.clone();
        reader_handles.push(thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            *worl_reader.read().unwrap()
        }));
    }

    thread::sleep(Duration::from_millis(30));
    *worl.write().unwrap() += 10;

    for handle in reader_handles {
        let value = handle.join().unwrap();
        assert!(value == 0 || value == 10);
    }

    let worl_clear = worl.clone();
    let clear_handle = thread::spawn(move || {
        worl_clear.write().unwrap().clear();
    });
    clear_handle.join().unwrap();

    assert!(worl.read().is_err());

    let worl_set = worl.clone();
    let set_handle = thread::spawn(move || {
        worl_set.set(91).unwrap();
    });
    set_handle.join().unwrap();

    let value = *worl.read().unwrap();
    assert_eq!(value, 91);

    let worl_clear = worl.clone();
    let clear_handle = thread::spawn(move || {
        worl_clear.write().unwrap().clear();
    });
    clear_handle.join().unwrap();

    assert!(worl.read().is_err());

    let worl_set = worl.clone();
    let set_handle = thread::spawn(move || {
        worl_set.set(91).unwrap();
    });
    set_handle.join().unwrap();

    let value = *worl.read().unwrap();
    assert_eq!(value, 91);
}

#[test]
fn test_wom_sync() {
    let empty_wom = Wom::<i32>::empty();
    assert!(empty_wom.get_mut().is_err());
    assert!(empty_wom.lock().is_err());

    let wom = Wom::new(10);

    {
        let guard = wom.get_mut().unwrap();
        assert_eq!(*guard, 10);
    }

    {
        let guard = wom.get_mut().unwrap();
        *guard = 15;
        *wom.get_mut().unwrap() = 17;
        assert_eq!(*guard, 17);
    }
    assert_eq!(*wom.get_mut().unwrap(), 17);

    {
        let guard = wom.lock().unwrap();
        assert_eq!(*guard, 17);
    }

    {
        let mut guard = wom.lock().unwrap();
        *guard += 3;
    }
    assert_eq!(*wom.lock().unwrap(), 20);

    {
        let guard = wom.lock().unwrap();
        assert!(wom.lock().is_err());
        assert_eq!(*guard, 20);
    }
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
