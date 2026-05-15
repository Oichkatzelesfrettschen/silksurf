//! Shape system (hidden classes) for object property optimization
//!
//! Shapes track object property layouts to enable:
//! - Efficient property access via fixed offsets
//! - Inline caching for property operations
//! - Memory sharing between objects with same structure
//!
//! Design informed by V8's Maps and `SpiderMonkey`'s Shapes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Property key - either a string (interned) or a symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyKey {
    /// Interned string index
    String(u32),
    /// Symbol index
    Symbol(u32),
}

/// Property attributes (ECMA-262 property descriptor flags)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PropertyAttributes {
    /// Property is writable
    pub writable: bool,
    /// Property is enumerable
    pub enumerable: bool,
    /// Property is configurable
    pub configurable: bool,
}

impl PropertyAttributes {
    /// Default data property attributes (writable, enumerable, configurable)
    pub const DEFAULT: Self = Self {
        writable: true,
        enumerable: true,
        configurable: true,
    };

    /// Non-writable, non-enumerable, non-configurable
    pub const FROZEN: Self = Self {
        writable: false,
        enumerable: false,
        configurable: false,
    };
}

/// A property descriptor within a shape
#[derive(Debug, Clone, Copy)]
pub struct PropertyDescriptor {
    /// Slot index in the object's property storage
    pub slot: u32,
    /// Property attributes
    pub attrs: PropertyAttributes,
}

/// Unique shape identifier
pub type ShapeId = u32;

/// A shape describes the property layout of a set of objects
///
/// Shapes form a transition tree:
/// - The root shape has no properties
/// - Each edge represents adding a property with specific attributes
/// - Objects with the same shape have the same property layout
#[derive(Debug)]
pub struct Shape {
    /// Unique identifier
    pub id: ShapeId,
    /// Parent shape (None for root)
    pub parent: Option<Rc<Shape>>,
    /// Property that was added from parent (None for root)
    pub added_property: Option<PropertyKey>,
    /// Property descriptors (key -> descriptor)
    /// Only includes properties added by this shape, not inherited from parent
    properties: HashMap<PropertyKey, PropertyDescriptor>,
    /// Total number of properties (including inherited)
    pub property_count: u32,
    /// Transitions to child shapes (key -> child shape)
    /// Using `RefCell` for interior mutability
    transitions: RefCell<HashMap<PropertyKey, Rc<Shape>>>,
    /// Is this shape for a frozen object?
    pub frozen: bool,
    /// Is this shape for a sealed object?
    pub sealed: bool,
}

impl Shape {
    /// Get property descriptor by key
    pub fn get_property(&self, key: PropertyKey) -> Option<PropertyDescriptor> {
        // Check this shape first
        if let Some(desc) = self.properties.get(&key) {
            return Some(*desc);
        }
        // Check parent chain
        if let Some(ref parent) = self.parent {
            parent.get_property(key)
        } else {
            None
        }
    }

    /// Check if shape has property
    pub fn has_property(&self, key: PropertyKey) -> bool {
        self.get_property(key).is_some()
    }

    /// Get all property keys in order (for enumeration)
    pub fn property_keys(&self) -> Vec<PropertyKey> {
        let mut keys = Vec::with_capacity(self.property_count as usize);
        self.collect_keys(&mut keys);
        keys
    }

    fn collect_keys(&self, keys: &mut Vec<PropertyKey>) {
        // Collect from parent first (for correct order)
        if let Some(ref parent) = self.parent {
            parent.collect_keys(keys);
        }
        // Then add this shape's property
        if let Some(key) = self.added_property {
            keys.push(key);
        }
    }

    /// Get cached transition or None
    pub fn get_transition(&self, key: PropertyKey) -> Option<Rc<Shape>> {
        self.transitions.borrow().get(&key).cloned()
    }

    /// Add a transition to a child shape
    pub fn add_transition(&self, key: PropertyKey, child: Rc<Shape>) {
        self.transitions.borrow_mut().insert(key, child);
    }
}

/// Shape table - manages all shapes in the engine
#[derive(Debug)]
pub struct ShapeTable {
    /// All shapes, indexed by ID
    shapes: Vec<Rc<Shape>>,
    /// Root shape (empty object)
    root: Rc<Shape>,
    /// Next shape ID
    next_id: ShapeId,
}

impl ShapeTable {
    /// Create a new shape table with root shape
    #[must_use]
    pub fn new() -> Self {
        let root = Rc::new(Shape {
            id: 0,
            parent: None,
            added_property: None,
            properties: HashMap::new(),
            property_count: 0,
            transitions: RefCell::new(HashMap::new()),
            frozen: false,
            sealed: false,
        });

        Self {
            shapes: vec![root.clone()],
            root,
            next_id: 1,
        }
    }

    /// Get the root (empty object) shape
    #[must_use]
    pub fn root(&self) -> Rc<Shape> {
        self.root.clone()
    }

    /// Get shape by ID
    #[must_use]
    pub fn get(&self, id: ShapeId) -> Option<Rc<Shape>> {
        self.shapes.get(id as usize).cloned()
    }

    /// Add a property to a shape, returning the new shape
    ///
    /// If a transition already exists, returns the cached shape.
    /// Otherwise creates a new shape and caches the transition.
    pub fn add_property(
        &mut self,
        shape: &Rc<Shape>,
        key: PropertyKey,
        attrs: PropertyAttributes,
    ) -> Rc<Shape> {
        // Check for existing transition
        if let Some(child) = shape.get_transition(key) {
            return child;
        }

        // Check if property already exists (can't add twice)
        if shape.has_property(key) {
            // Property exists - this would be a reconfiguration, not handled here
            return shape.clone();
        }

        // Create new shape
        let id = self.next_id;
        self.next_id += 1;

        let slot = shape.property_count;
        let mut properties = HashMap::new();
        properties.insert(key, PropertyDescriptor { slot, attrs });

        let new_shape = Rc::new(Shape {
            id,
            parent: Some(shape.clone()),
            added_property: Some(key),
            properties,
            property_count: shape.property_count + 1,
            transitions: RefCell::new(HashMap::new()),
            frozen: false,
            sealed: false,
        });

        // Cache the transition
        shape.add_transition(key, new_shape.clone());

        // Store in table
        self.shapes.push(new_shape.clone());

        new_shape
    }

    /// Number of shapes
    #[must_use]
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }
}

impl Default for ShapeTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Object with shape-based property storage
#[derive(Debug)]
pub struct ShapedObject {
    /// Shape describing property layout
    pub shape: Rc<Shape>,
    /// Property values (indexed by slot from shape)
    pub slots: Vec<u64>, // Using u64 for NaN-boxed values
}

impl ShapedObject {
    /// Create empty object
    pub fn new(shape: Rc<Shape>) -> Self {
        Self {
            shape,
            slots: Vec::new(),
        }
    }

    /// Get property value by key
    #[must_use]
    pub fn get(&self, key: PropertyKey) -> Option<u64> {
        let desc = self.shape.get_property(key)?;
        self.slots.get(desc.slot as usize).copied()
    }

    /// Set property value, potentially transitioning to new shape
    pub fn set(&mut self, table: &mut ShapeTable, key: PropertyKey, value: u64) {
        if let Some(desc) = self.shape.get_property(key) {
            // Property exists - update in place
            if (desc.slot as usize) < self.slots.len() {
                self.slots[desc.slot as usize] = value;
            }
        } else {
            // New property - transition to new shape
            let new_shape = table.add_property(&self.shape, key, PropertyAttributes::DEFAULT);
            self.shape = new_shape;
            self.slots.push(value);
        }
    }

    /// Check if object has property
    #[must_use]
    pub fn has(&self, key: PropertyKey) -> bool {
        self.shape.has_property(key)
    }

    /// Get enumerable property keys
    #[must_use]
    pub fn keys(&self) -> Vec<PropertyKey> {
        self.shape
            .property_keys()
            .into_iter()
            .filter(|k| {
                self.shape
                    .get_property(*k)
                    .is_some_and(|d| d.attrs.enumerable)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_table_creation() {
        let table = ShapeTable::new();
        assert_eq!(table.len(), 1); // Root shape
        assert_eq!(table.root().property_count, 0);
    }

    #[test]
    fn test_add_property() {
        let mut table = ShapeTable::new();
        let root = table.root();

        let key_x = PropertyKey::String(1); // "x"
        let shape1 = table.add_property(&root, key_x, PropertyAttributes::DEFAULT);

        assert_eq!(shape1.property_count, 1);
        assert!(shape1.has_property(key_x));

        let key_y = PropertyKey::String(2); // "y"
        let shape2 = table.add_property(&shape1, key_y, PropertyAttributes::DEFAULT);

        assert_eq!(shape2.property_count, 2);
        assert!(shape2.has_property(key_x));
        assert!(shape2.has_property(key_y));
    }

    #[test]
    fn test_transition_caching() {
        let mut table = ShapeTable::new();
        let root = table.root();

        let key_x = PropertyKey::String(1);

        // First addition
        let shape1a = table.add_property(&root, key_x, PropertyAttributes::DEFAULT);

        // Same transition should return cached shape
        let shape1b = table.add_property(&root, key_x, PropertyAttributes::DEFAULT);

        assert_eq!(shape1a.id, shape1b.id);
        assert_eq!(table.len(), 2); // Root + one child
    }

    #[test]
    fn test_property_descriptors() {
        let mut table = ShapeTable::new();
        let root = table.root();

        let key = PropertyKey::String(1);
        let shape = table.add_property(&root, key, PropertyAttributes::DEFAULT);

        // UNWRAP-OK: key was just added via add_property on the previous line, so get_property returns Some.
        let desc = shape.get_property(key).unwrap();
        assert_eq!(desc.slot, 0);
        assert!(desc.attrs.writable);
        assert!(desc.attrs.enumerable);
        assert!(desc.attrs.configurable);
    }

    #[test]
    fn test_shaped_object() {
        let mut table = ShapeTable::new();
        let mut obj = ShapedObject::new(table.root());

        let key_x = PropertyKey::String(1);
        let key_y = PropertyKey::String(2);

        // Set properties
        obj.set(&mut table, key_x, 42);
        obj.set(&mut table, key_y, 100);

        // Get properties
        assert_eq!(obj.get(key_x), Some(42));
        assert_eq!(obj.get(key_y), Some(100));

        // Update property
        obj.set(&mut table, key_x, 99);
        assert_eq!(obj.get(key_x), Some(99));

        // Check shape
        assert_eq!(obj.shape.property_count, 2);
    }

    #[test]
    fn test_property_keys() {
        let mut table = ShapeTable::new();

        let key_a = PropertyKey::String(1);
        let key_b = PropertyKey::String(2);
        let key_c = PropertyKey::String(3);

        let shape1 = table.add_property(&table.root(), key_a, PropertyAttributes::DEFAULT);
        let shape2 = table.add_property(&shape1, key_b, PropertyAttributes::DEFAULT);
        let shape3 = table.add_property(&shape2, key_c, PropertyAttributes::DEFAULT);

        let keys = shape3.property_keys();
        assert_eq!(keys, vec![key_a, key_b, key_c]);
    }

    #[test]
    fn test_multiple_objects_same_shape() {
        let mut table = ShapeTable::new();

        let key_x = PropertyKey::String(1);
        let key_y = PropertyKey::String(2);

        // Create first object: {x: 1, y: 2}
        let mut obj1 = ShapedObject::new(table.root());
        obj1.set(&mut table, key_x, 1);
        obj1.set(&mut table, key_y, 2);

        // Create second object with same shape: {x: 10, y: 20}
        let mut obj2 = ShapedObject::new(table.root());
        obj2.set(&mut table, key_x, 10);
        obj2.set(&mut table, key_y, 20);

        // Both should share the same shape
        assert_eq!(obj1.shape.id, obj2.shape.id);

        // But different values
        assert_eq!(obj1.get(key_x), Some(1));
        assert_eq!(obj2.get(key_x), Some(10));
    }
}
