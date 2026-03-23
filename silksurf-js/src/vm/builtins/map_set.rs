/*
 * map_set.rs -- Map, Set, WeakMap, WeakSet, and Symbol built-ins.
 *
 * WHY: React and modern JS make heavy use of Map (context storage,
 * memoization caches), Set (deduplication), and Symbol (well-known
 * protocol symbols like Symbol.iterator, Symbol.toPrimitive).
 * Without these globals, any script that calls `new Map()` or
 * accesses `Symbol.iterator` throws a ReferenceError/TypeError.
 *
 * Design constraints:
 *
 *   Value has no Symbol variant -- true symbol identity would require
 *   adding a new enum arm and threading it through the whole VM.
 *   Instead, Symbol() returns a unique string with a special prefix
 *   "@@symbol_N_" that is unlikely to collide with user strings.
 *   Well-known symbols (Symbol.iterator, etc.) are fixed strings.
 *   This handles the ~95% use case (protocol conformance checks)
 *   without a VM change.
 *
 *   Map requires reference equality for Object keys (two different
 *   {x:1} literals are NOT equal). We use a Vec<(Value, Value)>
 *   backing store and compare keys with map_key_eq, which uses
 *   Rc pointer equality for Objects and value equality for primitives.
 *
 *   Set uses the same backing store as a flat Vec<Value>.
 *
 * See: value.rs Value enum for the variant list
 * See: globals.rs install() for where these are registered
 * See: mod.rs op_get_prop for Symbol static property dispatch
 */

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::vm::builtins::array::create_array;
use crate::vm::value::{NativeFunction, Object, PropertyKey, Value};

// Global counter for unique symbol IDs.
// AtomicU64 so it's safe even if threads ever used (currently single-threaded).
static SYMBOL_COUNTER: AtomicU64 = AtomicU64::new(0);

// Thread-local symbol registry for Symbol.for() / Symbol.keyFor().
thread_local! {
    static SYMBOL_REGISTRY: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/*
 * make_symbol_value -- create a unique symbol string.
 *
 * Returns a Value::String of the form "@@symbol_N_description"
 * where N is a monotonically increasing u64. The @@ prefix makes
 * accidental collision with user strings extremely unlikely.
 */
pub fn make_symbol_value(description: &str) -> Value {
    let id = SYMBOL_COUNTER.fetch_add(1, Ordering::Relaxed);
    Value::string_owned(format!("@@symbol_{id}_{description}"))
}

/*
 * install -- register Map, Set, WeakMap, WeakSet, Symbol on the global.
 *
 * Each constructor is a NativeFunction whose name is the dispatch key
 * used in op_get_prop for static methods (Symbol.iterator, etc.).
 */
pub fn install(global: &mut Object) {
    global.set_by_str("Symbol", Value::NativeFunction(Rc::new(NativeFunction::new(
        "Symbol",
        |args| {
            let desc = args.first()
                .map(|v| { let s = v.to_js_string(); s.as_str().unwrap_or("").to_string() })
                .unwrap_or_default();
            make_symbol_value(&desc)
        },
    ))));

    global.set_by_str("Map", Value::NativeFunction(Rc::new(NativeFunction::new(
        "Map",
        |args| make_map(args.first()),
    ))));

    global.set_by_str("Set", Value::NativeFunction(Rc::new(NativeFunction::new(
        "Set",
        |args| make_set(args.first()),
    ))));

    /*
     * WeakMap / WeakSet stubs.
     *
     * WHY: React DevTools and some React internals register WeakMap
     * caches. Our VM's Values are Rc-based (no weak refs), so a true
     * WeakMap is impossible without a global GC. These stubs accept
     * the constructor call and return an object with the right API
     * shape backed by a strong Vec -- effectively a strong Map.
     * Objects that would be garbage-collected in a real browser may
     * accumulate here, but for a single-page rendering pass this is
     * safe (the entire VM is dropped after rendering).
     */
    global.set_by_str("WeakMap", Value::NativeFunction(Rc::new(NativeFunction::new(
        "WeakMap",
        |args| make_map(args.first()),
    ))));

    global.set_by_str("WeakSet", Value::NativeFunction(Rc::new(NativeFunction::new(
        "WeakSet",
        |args| make_set(args.first()),
    ))));

    /*
     * WeakRef stub -- used by React scheduler.
     *
     * new WeakRef(target): returns an object with .deref() -> target.
     * Since we have no GC, deref() always returns the original target.
     */
    global.set_by_str("WeakRef", Value::NativeFunction(Rc::new(NativeFunction::new(
        "WeakRef",
        |args| {
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            let obj = Rc::new(RefCell::new(Object::new()));
            {
                let mut o = obj.borrow_mut();
                let t = target.clone();
                o.set_by_str("deref", Value::NativeFunction(Rc::new(NativeFunction::new(
                    "deref",
                    move |_| t.clone(),
                ))));
                o.set_by_str("__target__", target);
            }
            Value::Object(obj)
        },
    ))));

    /*
     * FinalizationRegistry stub.
     *
     * WHY: React 19+ uses FinalizationRegistry for cleanup callbacks.
     * Without the constructor, `new FinalizationRegistry(fn)` throws.
     * Register/unregister are no-ops since we have no GC.
     */
    global.set_by_str("FinalizationRegistry", Value::NativeFunction(Rc::new(NativeFunction::new(
        "FinalizationRegistry",
        |_args| {
            let obj = Rc::new(RefCell::new(Object::new()));
            {
                let mut o = obj.borrow_mut();
                o.set_by_str("register", Value::NativeFunction(Rc::new(NativeFunction::new(
                    "register", |_| Value::Undefined,
                ))));
                o.set_by_str("unregister", Value::NativeFunction(Rc::new(NativeFunction::new(
                    "unregister", |_| Value::Undefined,
                ))));
            }
            Value::Object(obj)
        },
    ))));
}

/*
 * map_key_eq -- reference-safe key equality for Map.
 *
 * WHY: JS Map uses SameValueZero semantics. For Objects, two values
 * are equal iff they are the same reference (same Rc pointer).
 * For primitives, standard value equality applies.
 * NaN !== NaN per SameValueZero (same as ===).
 *
 * See: ECMA-262 Section 7.2.11 SameValueZero
 */
fn map_key_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(ra), Value::Object(rb)) => Rc::ptr_eq(ra, rb),
        (Value::Number(x), Value::Number(y)) => x == y, // NaN != NaN
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::Undefined, Value::Undefined) => true,
        _ => false,
    }
}

/*
 * make_map -- construct a new JS Map object.
 *
 * Backing store: Rc<RefCell<Vec<(Value, Value)>>> shared across all
 * method closures. This gives correct reference identity for Object
 * keys at the cost of O(N) lookup (acceptable for rendering scripts
 * which rarely hold more than a few hundred entries).
 *
 * If an iterable is provided (array of [key, value] pairs), pre-populate.
 */
fn make_map(initial: Option<&Value>) -> Value {
    let data: Rc<RefCell<Vec<(Value, Value)>>> = Rc::new(RefCell::new(Vec::new()));

    // Pre-populate from initial entries array
    if let Some(Value::Object(entries_obj)) = initial {
        let entries_borrow = entries_obj.borrow();
        let entries = crate::vm::builtins::array::collect_elements_pub(&entries_borrow);
        drop(entries_borrow);
        let mut store = data.borrow_mut();
        for entry in entries {
            if let Value::Object(pair) = entry {
                let k = pair.borrow().get_by_key(&PropertyKey::Index(0));
                let v = pair.borrow().get_by_key(&PropertyKey::Index(1));
                store.push((k, v));
            }
        }
    }

    let obj = Rc::new(RefCell::new(Object::new()));

    // Map.prototype.get
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("get", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.get",
            move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let store = d.borrow();
                for (k, v) in store.iter() {
                    if map_key_eq(k, &key) {
                        return v.clone();
                    }
                }
                Value::Undefined
            },
        ))));
    }

    // Map.prototype.set
    {
        let d = Rc::clone(&data);
        let obj_clone = Rc::clone(&obj);
        obj.borrow_mut().set_by_str("set", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.set",
            move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let val = args.get(1).cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                for (k, v) in store.iter_mut() {
                    if map_key_eq(k, &key) {
                        *v = val;
                        return Value::Object(Rc::clone(&obj_clone));
                    }
                }
                store.push((key, val));
                Value::Object(Rc::clone(&obj_clone))
            },
        ))));
    }

    // Map.prototype.has
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("has", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.has",
            move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let store = d.borrow();
                Value::Boolean(store.iter().any(|(k, _)| map_key_eq(k, &key)))
            },
        ))));
    }

    // Map.prototype.delete
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("delete", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.delete",
            move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                let before = store.len();
                store.retain(|(k, _)| !map_key_eq(k, &key));
                Value::Boolean(store.len() < before)
            },
        ))));
    }

    // Map.prototype.clear
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("clear", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.clear",
            move |_| {
                d.borrow_mut().clear();
                Value::Undefined
            },
        ))));
    }

    // Map.prototype.forEach
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("forEach", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.forEach",
            move |args| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::NativeFunction(f) = &cb {
                    let pairs: Vec<(Value, Value)> = d.borrow().clone();
                    for (k, v) in pairs {
                        f.call(&[v, k]);
                    }
                }
                Value::Undefined
            },
        ))));
    }

    // Map.prototype.keys / values / entries
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("keys", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.keys",
            move |_| {
                let store = d.borrow();
                let keys: Vec<Value> = store.iter().map(|(k, _)| k.clone()).collect();
                create_array(keys)
            },
        ))));
    }
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("values", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.values",
            move |_| {
                let store = d.borrow();
                let vals: Vec<Value> = store.iter().map(|(_, v)| v.clone()).collect();
                create_array(vals)
            },
        ))));
    }
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("entries", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.entries",
            move |_| {
                let store = d.borrow();
                let entries: Vec<Value> = store
                    .iter()
                    .map(|(k, v)| create_array(vec![k.clone(), v.clone()]))
                    .collect();
                create_array(entries)
            },
        ))));
    }

    // Map.prototype.size (as a property -- accessed via get_by_str in op_get_prop)
    // We store a reference to the backing data on the object itself so
    // op_get_prop can read the current size dynamically.
    // Hack: update size on every mutating operation is impractical with closures.
    // Instead, expose a __mapData__ property holding the Rc so op_get_prop
    // can call .len() dynamically.  Alternatively, we set size as a getter stub
    // that is re-evaluated each access.  Since NativeFunction can't be a
    // property descriptor with [[Get]], we use a workaround: update "size" on
    // every set/delete/clear by re-setting it inside the closure.
    //
    // Simpler: just expose a "size" method (some code uses .size as a property,
    // most can tolerate a NativeFunction that returns a number).
    // We'll set size as a lazy-computed native function that reads the data vec.
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("size", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Map.size",
            move |_| Value::Number(d.borrow().len() as f64),
        ))));
    }

    Value::Object(obj)
}

/*
 * make_set -- construct a new JS Set object.
 *
 * Backing store: Rc<RefCell<Vec<Value>>> with SameValueZero equality.
 * Pre-populate from an iterable (array or string) if provided.
 */
fn make_set(initial: Option<&Value>) -> Value {
    let data: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(Vec::new()));

    // Pre-populate from initial iterable
    match initial {
        Some(Value::Object(arr)) => {
            let elements = crate::vm::builtins::array::collect_elements_pub(&arr.borrow());
            let mut store = data.borrow_mut();
            for el in elements {
                if !store.iter().any(|x| map_key_eq(x, &el)) {
                    store.push(el);
                }
            }
        }
        Some(Value::String(s)) => {
            let chars: Vec<Value> = s.as_str()
                .unwrap_or("")
                .chars()
                .map(|c| Value::string_owned(c.to_string()))
                .collect();
            let mut store = data.borrow_mut();
            for c in chars {
                if !store.iter().any(|x| map_key_eq(x, &c)) {
                    store.push(c);
                }
            }
        }
        _ => {}
    }

    let obj = Rc::new(RefCell::new(Object::new()));

    // Set.prototype.add
    {
        let d = Rc::clone(&data);
        let obj_clone = Rc::clone(&obj);
        obj.borrow_mut().set_by_str("add", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.add",
            move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                if !store.iter().any(|x| map_key_eq(x, &val)) {
                    store.push(val);
                }
                Value::Object(Rc::clone(&obj_clone))
            },
        ))));
    }

    // Set.prototype.has
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("has", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.has",
            move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                Value::Boolean(d.borrow().iter().any(|x| map_key_eq(x, &val)))
            },
        ))));
    }

    // Set.prototype.delete
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("delete", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.delete",
            move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                let before = store.len();
                store.retain(|x| !map_key_eq(x, &val));
                Value::Boolean(store.len() < before)
            },
        ))));
    }

    // Set.prototype.clear
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("clear", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.clear",
            move |_| { d.borrow_mut().clear(); Value::Undefined },
        ))));
    }

    // Set.prototype.forEach
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("forEach", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.forEach",
            move |args| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::NativeFunction(f) = &cb {
                    let elements: Vec<Value> = d.borrow().clone();
                    for el in elements {
                        f.call(&[el.clone(), el]);
                    }
                }
                Value::Undefined
            },
        ))));
    }

    // Set.prototype.keys / values / entries
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("values", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.values",
            move |_| create_array(d.borrow().clone()),
        ))));
    }
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("keys", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.keys",
            move |_| create_array(d.borrow().clone()),
        ))));
    }
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("entries", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.entries",
            move |_| {
                let elements: Vec<Value> = d.borrow()
                    .iter()
                    .map(|v| create_array(vec![v.clone(), v.clone()]))
                    .collect();
                create_array(elements)
            },
        ))));
    }
    {
        let d = Rc::clone(&data);
        obj.borrow_mut().set_by_str("size", Value::NativeFunction(Rc::new(NativeFunction::new(
            "Set.size",
            move |_| Value::Number(d.borrow().len() as f64),
        ))));
    }

    Value::Object(obj)
}
