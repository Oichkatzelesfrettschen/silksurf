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
use std::collections::{HashMap, HashSet};
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
    global.set_by_str(
        "Symbol",
        Value::NativeFunction(Rc::new(NativeFunction::new("Symbol", |args| {
            let desc = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            make_symbol_value(&desc)
        }))),
    );

    global.set_by_str(
        "Map",
        Value::NativeFunction(Rc::new(NativeFunction::new("Map", |args| {
            make_map(args.first())
        }))),
    );

    global.set_by_str(
        "Set",
        Value::NativeFunction(Rc::new(NativeFunction::new("Set", |args| {
            make_set(args.first())
        }))),
    );

    /*
     * WeakMap / WeakSet -- pseudo-weak, object-identity keyed.
     *
     * WHY: React DevTools and several React internals (effect-cleanup
     * registries, component caches) require WeakMap/WeakSet semantics:
     * keys must be objects, and equality is reference identity, not
     * structural. The earlier shims aliased these to ordinary Map/Set,
     * which silently accepted primitive keys and used SameValueZero --
     * masking bugs and producing the wrong answer when caller code
     * relied on `weakmap.has({})` returning false.
     *
     * Design -- pseudo-weak keying:
     *
     *   The key is the raw allocation address of the key object's
     *   `Rc<RefCell<Object>>`, captured as `usize` via Rc::as_ptr.
     *   Two `Value::Object` values share a key iff they are the same
     *   Rc allocation, which is precisely JS reference identity.
     *   The backing store is `HashMap<usize, Value>` (O(1) lookup),
     *   not the Vec scan used by Map (O(N)).
     *
     *   "Pseudo" because we do not (yet) prune entries when the key
     *   object is dropped -- a true WeakMap requires a tracing GC to
     *   observe key reachability, which this VM does not have. Until
     *   gc_integration.rs gains a live GC, dead entries linger.
     *
     *   Hazard -- address reuse: if the user drops the last strong
     *   reference to a key, the Rc deallocates and a subsequently
     *   constructed Object may land at the same address. A lookup
     *   with that new object would then spuriously hit the stale
     *   entry. This is acceptable for the React rendering use case
     *   (the entire VM is torn down between renders, so no long-lived
     *   accumulation), but a real GC must replace this scheme before
     *   long-running scripts can rely on it.
     *
     *   Spec divergence -- non-Object keys: ECMA-262 requires `set`,
     *   `has`, `delete`, `add` to throw TypeError on primitive keys.
     *   This VM's NativeFunction signature `Fn(&[Value]) -> Value`
     *   has no error channel, so we silently no-op (set/add) or
     *   return false (has/delete). Callers that depend on the throw
     *   will see incorrect behavior; this is documented and will be
     *   fixed when the VM grows native-throw support.
     *
     * See: gc_integration.rs for the future weak-ref plumbing
     * See: make_weak_map / make_weak_set below
     */
    global.set_by_str(
        "WeakMap",
        Value::NativeFunction(Rc::new(NativeFunction::new("WeakMap", |args| {
            make_weak_map(args.first())
        }))),
    );

    global.set_by_str(
        "WeakSet",
        Value::NativeFunction(Rc::new(NativeFunction::new("WeakSet", |args| {
            make_weak_set(args.first())
        }))),
    );

    /*
     * WeakRef stub -- used by React scheduler.
     *
     * new WeakRef(target): returns an object with .deref() -> target.
     * Since we have no GC, deref() always returns the original target.
     */
    global.set_by_str(
        "WeakRef",
        Value::NativeFunction(Rc::new(NativeFunction::new("WeakRef", |args| {
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            let obj = Rc::new(RefCell::new(Object::new()));
            {
                let mut o = obj.borrow_mut();
                let t = target.clone();
                o.set_by_str(
                    "deref",
                    Value::NativeFunction(Rc::new(NativeFunction::new("deref", move |_| {
                        t.clone()
                    }))),
                );
                o.set_by_str("__target__", target);
            }
            Value::Object(obj)
        }))),
    );

    /*
     * FinalizationRegistry stub.
     *
     * WHY: React 19+ uses FinalizationRegistry for cleanup callbacks.
     * Without the constructor, `new FinalizationRegistry(fn)` throws.
     * Register/unregister are no-ops since we have no GC.
     */
    global.set_by_str(
        "FinalizationRegistry",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            "FinalizationRegistry",
            |_args| {
                let obj = Rc::new(RefCell::new(Object::new()));
                {
                    let mut o = obj.borrow_mut();
                    o.set_by_str(
                        "register",
                        Value::NativeFunction(Rc::new(NativeFunction::new("register", |_| {
                            Value::Undefined
                        }))),
                    );
                    o.set_by_str(
                        "unregister",
                        Value::NativeFunction(Rc::new(NativeFunction::new("unregister", |_| {
                            Value::Undefined
                        }))),
                    );
                }
                Value::Object(obj)
            },
        ))),
    );
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
        // JS SameValueZero: exact IEEE 754 comparison (NaN != NaN, +0 == -0).
        // Epsilon comparison would violate ECMA-262 SameValueZero semantics.
        #[allow(clippy::float_cmp)]
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
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
    let map_entries: Rc<RefCell<Vec<(Value, Value)>>> = Rc::new(RefCell::new(Vec::new()));

    // Pre-populate from initial entries array
    if let Some(Value::Object(entries_obj)) = initial {
        let entries_borrow = entries_obj.borrow();
        let entries = crate::vm::builtins::array::collect_elements_pub(&entries_borrow);
        drop(entries_borrow);
        let mut store = map_entries.borrow_mut();
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
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "get",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.get", move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let store = d.borrow();
                for (k, v) in store.iter() {
                    if map_key_eq(k, &key) {
                        return v.clone();
                    }
                }
                Value::Undefined
            }))),
        );
    }

    // Map.prototype.set
    {
        let d = Rc::clone(&map_entries);
        let obj_clone = Rc::clone(&obj);
        obj.borrow_mut().set_by_str(
            "set",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.set", move |args| {
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
            }))),
        );
    }

    // Map.prototype.has
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "has",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.has", move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let store = d.borrow();
                Value::Boolean(store.iter().any(|(k, _)| map_key_eq(k, &key)))
            }))),
        );
    }

    // Map.prototype.delete
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "delete",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.delete", move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                let before = store.len();
                store.retain(|(k, _)| !map_key_eq(k, &key));
                Value::Boolean(store.len() < before)
            }))),
        );
    }

    // Map.prototype.clear
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "clear",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.clear", move |_| {
                d.borrow_mut().clear();
                Value::Undefined
            }))),
        );
    }

    // Map.prototype.forEach
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "forEach",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.forEach", move |args| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::NativeFunction(f) = &cb {
                    let pairs: Vec<(Value, Value)> = d.borrow().clone();
                    for (k, v) in pairs {
                        let _ = f.call(&[v, k]);
                    }
                }
                Value::Undefined
            }))),
        );
    }

    // Map.prototype.keys / values / entries
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "keys",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.keys", move |_| {
                let store = d.borrow();
                let keys: Vec<Value> = store.iter().map(|(k, _)| k.clone()).collect();
                create_array(&keys)
            }))),
        );
    }
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "values",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.values", move |_| {
                let store = d.borrow();
                let vals: Vec<Value> = store.iter().map(|(_, v)| v.clone()).collect();
                create_array(&vals)
            }))),
        );
    }
    {
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "entries",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.entries", move |_| {
                let store = d.borrow();
                let entries: Vec<Value> = store
                    .iter()
                    .map(|(k, v)| create_array(&[k.clone(), v.clone()]))
                    .collect();
                create_array(&entries)
            }))),
        );
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
        let d = Rc::clone(&map_entries);
        obj.borrow_mut().set_by_str(
            "size",
            Value::NativeFunction(Rc::new(NativeFunction::new("Map.size", move |_| {
                Value::Number(d.borrow().len() as f64)
            }))),
        );
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
    let set_items: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(Vec::new()));

    // Pre-populate from initial iterable
    match initial {
        Some(Value::Object(arr)) => {
            let elements = crate::vm::builtins::array::collect_elements_pub(&arr.borrow());
            let mut store = set_items.borrow_mut();
            for el in elements {
                if !store.iter().any(|x| map_key_eq(x, &el)) {
                    store.push(el);
                }
            }
        }
        Some(Value::String(s)) => {
            let chars: Vec<Value> = s
                .as_str()
                .unwrap_or("")
                .chars()
                .map(|c| Value::string_owned(c.to_string()))
                .collect();
            let mut store = set_items.borrow_mut();
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
        let d = Rc::clone(&set_items);
        let obj_clone = Rc::clone(&obj);
        obj.borrow_mut().set_by_str(
            "add",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.add", move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                if !store.iter().any(|x| map_key_eq(x, &val)) {
                    store.push(val);
                }
                Value::Object(Rc::clone(&obj_clone))
            }))),
        );
    }

    // Set.prototype.has
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "has",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.has", move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                Value::Boolean(d.borrow().iter().any(|x| map_key_eq(x, &val)))
            }))),
        );
    }

    // Set.prototype.delete
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "delete",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.delete", move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let mut store = d.borrow_mut();
                let before = store.len();
                store.retain(|x| !map_key_eq(x, &val));
                Value::Boolean(store.len() < before)
            }))),
        );
    }

    // Set.prototype.clear
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "clear",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.clear", move |_| {
                d.borrow_mut().clear();
                Value::Undefined
            }))),
        );
    }

    // Set.prototype.forEach
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "forEach",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.forEach", move |args| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::NativeFunction(f) = &cb {
                    let elements: Vec<Value> = d.borrow().clone();
                    for el in elements {
                        let _ = f.call(&[el.clone(), el]);
                    }
                }
                Value::Undefined
            }))),
        );
    }

    // Set.prototype.keys / values / entries
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "values",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.values", move |_| {
                create_array(&d.borrow().clone())
            }))),
        );
    }
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "keys",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.keys", move |_| {
                create_array(&d.borrow().clone())
            }))),
        );
    }
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "entries",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.entries", move |_| {
                let elements: Vec<Value> = d
                    .borrow()
                    .iter()
                    .map(|v| create_array(&[v.clone(), v.clone()]))
                    .collect();
                create_array(&elements)
            }))),
        );
    }
    {
        let d = Rc::clone(&set_items);
        obj.borrow_mut().set_by_str(
            "size",
            Value::NativeFunction(Rc::new(NativeFunction::new("Set.size", move |_| {
                Value::Number(d.borrow().len() as f64)
            }))),
        );
    }

    Value::Object(obj)
}

/*
 * object_identity -- extract the pseudo-weak key for a Value.
 *
 * Returns Some(addr) iff the value is an Object whose Rc allocation
 * address can serve as an identity key. All other variants yield None
 * (the WeakMap/WeakSet methods treat None as "not a valid key").
 *
 * WHY usize, not Rc: storing the Rc would keep the key object alive,
 * which is the opposite of weak semantics. The address is observed,
 * not owned. See the pseudo-weak hazard note above install().
 */
fn object_identity(value: &Value) -> Option<usize> {
    if let Value::Object(rc) = value {
        Some(Rc::as_ptr(rc) as usize)
    } else {
        None
    }
}

/*
 * make_weak_map -- construct a JS WeakMap with object-identity keying.
 *
 * Backing store: Rc<RefCell<HashMap<usize, Value>>> shared by every
 * method closure. Each method extracts the key's identity address and
 * indexes the hash map; lookups are O(1) amortized.
 *
 * Methods (per ECMA-262 24.3):
 *   get(key)        -> stored value, or undefined if absent / bad key
 *   set(key, value) -> the WeakMap itself (for chaining)
 *   has(key)        -> boolean presence
 *   delete(key)     -> true if removed, false otherwise
 *
 * Optional initializer: an Array of [key, value] pairs, matching the
 * Map convention. Non-Object keys in the initializer are skipped.
 */
fn make_weak_map(initial: Option<&Value>) -> Value {
    let store: Rc<RefCell<HashMap<usize, Value>>> = Rc::new(RefCell::new(HashMap::new()));

    // Pre-populate from an array of [key, value] pairs.
    if let Some(Value::Object(entries_obj)) = initial {
        let entries_borrow = entries_obj.borrow();
        let entries = crate::vm::builtins::array::collect_elements_pub(&entries_borrow);
        drop(entries_borrow);
        let mut init_store = store.borrow_mut();
        for entry in entries {
            if let Value::Object(pair) = entry {
                let key_val = pair.borrow().get_by_key(&PropertyKey::Index(0));
                let val = pair.borrow().get_by_key(&PropertyKey::Index(1));
                if let Some(addr) = object_identity(&key_val) {
                    init_store.insert(addr, val);
                }
            }
        }
    }

    let obj = Rc::new(RefCell::new(Object::new()));

    // WeakMap.prototype.get
    {
        let d = Rc::clone(&store);
        obj.borrow_mut().set_by_str(
            "get",
            Value::NativeFunction(Rc::new(NativeFunction::new("WeakMap.get", move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let Some(addr) = object_identity(&key) else {
                    return Value::Undefined;
                };
                d.borrow().get(&addr).cloned().unwrap_or(Value::Undefined)
            }))),
        );
    }

    // WeakMap.prototype.set -- chainable (returns the WeakMap itself).
    {
        let d = Rc::clone(&store);
        let obj_clone = Rc::clone(&obj);
        obj.borrow_mut().set_by_str(
            "set",
            Value::NativeFunction(Rc::new(NativeFunction::new("WeakMap.set", move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let val = args.get(1).cloned().unwrap_or(Value::Undefined);
                // Spec: throw TypeError for non-Object key. No throw channel
                // available -- silently no-op so the chain still works.
                if let Some(addr) = object_identity(&key) {
                    d.borrow_mut().insert(addr, val);
                }
                Value::Object(Rc::clone(&obj_clone))
            }))),
        );
    }

    // WeakMap.prototype.has
    {
        let d = Rc::clone(&store);
        obj.borrow_mut().set_by_str(
            "has",
            Value::NativeFunction(Rc::new(NativeFunction::new("WeakMap.has", move |args| {
                let key = args.first().cloned().unwrap_or(Value::Undefined);
                let Some(addr) = object_identity(&key) else {
                    return Value::Boolean(false);
                };
                Value::Boolean(d.borrow().contains_key(&addr))
            }))),
        );
    }

    // WeakMap.prototype.delete
    {
        let d = Rc::clone(&store);
        obj.borrow_mut().set_by_str(
            "delete",
            Value::NativeFunction(Rc::new(NativeFunction::new(
                "WeakMap.delete",
                move |args| {
                    let key = args.first().cloned().unwrap_or(Value::Undefined);
                    let Some(addr) = object_identity(&key) else {
                        return Value::Boolean(false);
                    };
                    Value::Boolean(d.borrow_mut().remove(&addr).is_some())
                },
            ))),
        );
    }

    Value::Object(obj)
}

/*
 * make_weak_set -- construct a JS WeakSet with object-identity membership.
 *
 * Backing store: Rc<RefCell<HashSet<usize>>>. Same identity scheme and
 * same hazards as make_weak_map.
 *
 * Methods (per ECMA-262 24.4):
 *   add(value)    -> the WeakSet itself (for chaining)
 *   has(value)    -> boolean membership
 *   delete(value) -> true if removed, false otherwise
 *
 * Optional initializer: an Array of objects. Non-Object elements skipped.
 */
fn make_weak_set(initial: Option<&Value>) -> Value {
    let store: Rc<RefCell<HashSet<usize>>> = Rc::new(RefCell::new(HashSet::new()));

    if let Some(Value::Object(arr)) = initial {
        let arr_borrow = arr.borrow();
        let elements = crate::vm::builtins::array::collect_elements_pub(&arr_borrow);
        drop(arr_borrow);
        let mut init_store = store.borrow_mut();
        for el in elements {
            if let Some(addr) = object_identity(&el) {
                init_store.insert(addr);
            }
        }
    }

    let obj = Rc::new(RefCell::new(Object::new()));

    // WeakSet.prototype.add -- chainable.
    {
        let d = Rc::clone(&store);
        let obj_clone = Rc::clone(&obj);
        obj.borrow_mut().set_by_str(
            "add",
            Value::NativeFunction(Rc::new(NativeFunction::new("WeakSet.add", move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                // Spec: TypeError for non-Object. No throw channel -- no-op.
                if let Some(addr) = object_identity(&val) {
                    d.borrow_mut().insert(addr);
                }
                Value::Object(Rc::clone(&obj_clone))
            }))),
        );
    }

    // WeakSet.prototype.has
    {
        let d = Rc::clone(&store);
        obj.borrow_mut().set_by_str(
            "has",
            Value::NativeFunction(Rc::new(NativeFunction::new("WeakSet.has", move |args| {
                let val = args.first().cloned().unwrap_or(Value::Undefined);
                let Some(addr) = object_identity(&val) else {
                    return Value::Boolean(false);
                };
                Value::Boolean(d.borrow().contains(&addr))
            }))),
        );
    }

    // WeakSet.prototype.delete
    {
        let d = Rc::clone(&store);
        obj.borrow_mut().set_by_str(
            "delete",
            Value::NativeFunction(Rc::new(NativeFunction::new(
                "WeakSet.delete",
                move |args| {
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    let Some(addr) = object_identity(&val) else {
                        return Value::Boolean(false);
                    };
                    Value::Boolean(d.borrow_mut().remove(&addr))
                },
            ))),
        );
    }

    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    //! `WeakMap` / `WeakSet` pseudo-weak keying tests.
    //!
    //! WHY: These tests pin down object-identity keying for the new
    //! `WeakMap` / `WeakSet` implementations.  The earlier stubs aliased
    //! both to `Map` / `Set`, which silently accepted primitive keys and
    //! used `SameValueZero`; both properties are forbidden by spec.
    //!
    //! Each test compiles a small script through the full pipeline
    //! (parser -> compiler -> VM) and asserts the value left in
    //! `window.result`.  See vm/mod.rs `run_and_get_result` for the
    //! same helper pattern; we re-implement it locally to avoid a
    //! cross-module dependency on a `#[cfg(test)]` helper.

    use crate::bytecode::{Compiler, Constant};
    use crate::parser::Parser;
    use crate::parser::ast_arena::AstArena;
    use crate::vm::Vm;
    use crate::vm::value::Value;
    use std::collections::HashMap as StdHashMap;

    /// Compile and execute `source` against a fresh VM.
    fn execute(source: &str) -> Value {
        let arena = AstArena::new();
        let parser = Parser::new(source, &arena);
        let (ast, errors) = parser.parse();
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let compiler = Compiler::new();
        // UNWRAP-OK: compile_with_children only fails for malformed AST,
        // which our literal scripts cannot produce.
        let (chunk, child_chunks, string_pool) = compiler
            .compile_with_children(&ast)
            .expect("compile failed");
        let mut vm = Vm::new();
        let mut str_map: StdHashMap<u32, u32> = StdHashMap::new();
        for (compiler_id, s) in &string_pool {
            let vm_id = vm.strings.intern(s.clone());
            str_map.insert(*compiler_id, vm_id);
        }
        let child_base = vm.chunks_len();
        for mut child in child_chunks {
            for constant in child.constants_mut() {
                if let Constant::String(str_id) = constant
                    && let Some(&vm_id) = str_map.get(str_id)
                {
                    *str_id = vm_id;
                }
            }
            vm.add_chunk(child);
        }
        let mut main_chunk = chunk;
        for constant in main_chunk.constants_mut() {
            match constant {
                Constant::Function(idx) => *idx += child_base as u32,
                Constant::String(str_id) => {
                    if let Some(&vm_id) = str_map.get(str_id) {
                        *str_id = vm_id;
                    }
                }
                _ => {}
            }
        }
        let chunk_idx = vm.add_chunk(main_chunk);
        // UNWRAP-OK: literal scripts are well-formed; a failure here
        // would indicate the VM core regressed, not the WeakMap logic.
        vm.execute(chunk_idx).expect("execute failed");
        vm.global.borrow().get_by_str("result").clone()
    }

    fn as_number(v: &Value) -> f64 {
        if let Value::Number(n) = v {
            *n
        } else {
            panic!("expected number, got {v:?}")
        }
    }

    #[test]
    fn weakmap_set_and_get_returns_stored_value() {
        let v = execute(
            "var m = new WeakMap();\
             var k = {};\
             m.set(k, 42);\
             window.result = m.get(k);",
        );
        assert!((as_number(&v) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakmap_has_returns_true_for_present_key() {
        let v = execute(
            "var m = new WeakMap();\
             var k = {};\
             m.set(k, 1);\
             window.result = m.has(k) ? 1 : 0;",
        );
        assert!((as_number(&v) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakmap_distinct_object_literals_do_not_collide() {
        // Two `{}` literals allocate two distinct Rc objects: the
        // pseudo-weak key (Rc::as_ptr) must differ, so has(b) is false.
        let v = execute(
            "var m = new WeakMap();\
             var a = {};\
             var b = {};\
             m.set(a, 10);\
             window.result = m.has(b) ? 1 : 0;",
        );
        assert!((as_number(&v) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakmap_delete_returns_true_then_false() {
        let v = execute(
            "var m = new WeakMap();\
             var k = {};\
             m.set(k, 1);\
             var first = m.delete(k);\
             var second = m.delete(k);\
             window.result = (first && !second) ? 1 : 0;",
        );
        assert!((as_number(&v) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakmap_get_returns_undefined_for_primitive_key() {
        // Spec: TypeError; we lack a throw channel so silently return undefined.
        // The point is that primitive keys never accidentally hit any entry.
        let v = execute(
            "var m = new WeakMap();\
             m.set(\"hello\", 1);\
             window.result = (m.get(\"hello\") === undefined) ? 1 : 0;",
        );
        assert!((as_number(&v) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakset_add_and_has_returns_true() {
        let v = execute(
            "var s = new WeakSet();\
             var k = {};\
             s.add(k);\
             window.result = s.has(k) ? 1 : 0;",
        );
        assert!((as_number(&v) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakset_distinct_object_literals_do_not_collide() {
        let v = execute(
            "var s = new WeakSet();\
             var a = {};\
             var b = {};\
             s.add(a);\
             window.result = s.has(b) ? 1 : 0;",
        );
        assert!((as_number(&v) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weakset_delete_returns_true_then_false() {
        let v = execute(
            "var s = new WeakSet();\
             var k = {};\
             s.add(k);\
             var first = s.delete(k);\
             var second = s.delete(k);\
             window.result = (first && !second) ? 1 : 0;",
        );
        assert!((as_number(&v) - 1.0).abs() < f64::EPSILON);
    }
}
