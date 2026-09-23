(function () {
    'use strict';
    const collections = new WeakMap();
    const states = new WeakMap();
    function HTMLCollection() { throw new TypeError('Illegal constructor'); }
    function HTMLFormControlsCollection() { throw new TypeError('Illegal constructor'); }
    function RadioNodeList() { throw new TypeError('Illegal constructor'); }
    Object.setPrototypeOf(HTMLFormControlsCollection.prototype, HTMLCollection.prototype);
    Object.setPrototypeOf(RadioNodeList.prototype, NodeList.prototype);

    function members(collection) {
        const state = states.get(collection);
        if (!state) { throw new TypeError('Invalid collection receiver'); }
        const nodes = __silksurfFormControls(state.form.nodeId);
        return state.name === null ? nodes : nodes.filter(function (node) {
            return node.id === state.name || node.getAttribute('name') === state.name;
        });
    }
    function indexOfProperty(property) {
        if (typeof property !== 'string' || !/^(0|[1-9][0-9]*)$/.test(property)) { return null; }
        const index = Number(property);
        return index < 4294967295 ? index : null;
    }
    function named(collection, name) {
        name = String(name);
        if (name === '') { return null; }
        const state = states.get(collection);
        if (!state) { throw new TypeError('Invalid collection receiver'); }
        const matches = members(collection).filter(function (node) {
            return node.id === name || node.getAttribute('name') === name;
        });
        if (matches.length === 0) { return null; }
        if (matches.length === 1) { return matches[0]; }
        return create(state.form, name);
    }
    function supportedNames(collection) {
        const names = [];
        members(collection).forEach(function (node) {
            [node.id, node.getAttribute('name')].forEach(function (name) {
                if (name && names.indexOf(name) === -1) { names.push(name); }
            });
        });
        return names;
    }
    function create(form, name) {
        const prototype = name === null ? HTMLFormControlsCollection.prototype : RadioNodeList.prototype;
        const target = Object.create(prototype);
        const state = { form: form, name: name };
        const proxy = new Proxy(target, {
            get: function (target, property, receiver) {
                const index = indexOfProperty(property);
                if (index !== null) { return members(proxy)[index]; }
                if (Reflect.has(target, property)) { return Reflect.get(target, property, receiver); }
                return name === null && typeof property === 'string' ? named(proxy, property) || undefined : undefined;
            },
            has: function (target, property) {
                const index = indexOfProperty(property);
                if (index !== null) { return index < members(proxy).length; }
                return Reflect.has(target, property) || (name === null && typeof property === 'string' && named(proxy, property) !== null);
            },
            ownKeys: function () {
                const keys = Reflect.ownKeys(target);
                members(proxy).forEach(function (_, index) {
                    if (keys.indexOf(String(index)) === -1) { keys.push(String(index)); }
                });
                if (name === null) {
                    supportedNames(proxy).forEach(function (key) {
                        if (!(key in target) && keys.indexOf(key) === -1) { keys.push(key); }
                    });
                }
                return keys;
            },
            getOwnPropertyDescriptor: function (target, property) {
                const own = Reflect.getOwnPropertyDescriptor(target, property);
                if (own) { return own; }
                const index = indexOfProperty(property);
                const value = index !== null ? members(proxy)[index] :
                    name === null && typeof property === 'string' && !(property in target) ? named(proxy, property) : null;
                if (value === null || value === undefined) { return undefined; }
                return { value: value, writable: false, enumerable: index !== null, configurable: true };
            },
            defineProperty: function (target, property, descriptor) {
                if (indexOfProperty(property) !== null || (name === null && typeof property === 'string' && !(property in target) && named(proxy, property) !== null)) { return false; }
                return Reflect.defineProperty(target, property, descriptor);
            },
            deleteProperty: function (target, property) {
                if (indexOfProperty(property) !== null && indexOfProperty(property) < members(proxy).length) { return false; }
                return Reflect.deleteProperty(target, property);
            },
            preventExtensions: function () { return false; }
        });
        states.set(target, state);
        states.set(proxy, state);
        return proxy;
    }
    function item(index) { return members(this)[Number(index) >>> 0] || null; }
    function iterator() {
        const collection = this;
        let index = 0;
        return {
            next: function () {
                const nodes = members(collection);
                return index < nodes.length ? { value: nodes[index++], done: false } : { value: undefined, done: true };
            },
            [Symbol.iterator]: function () { return this; }
        };
    }
    [HTMLCollection.prototype, RadioNodeList.prototype].forEach(function (prototype) {
        Object.defineProperty(prototype, 'length', { get: function () { return members(this).length; }, configurable: true });
        prototype.item = item;
        prototype[Symbol.iterator] = iterator;
    });
    HTMLFormControlsCollection.prototype.namedItem = function (name) { return named(this, name); };
    HTMLCollection.prototype.namedItem = function (name) {
        name = String(name);
        return members(this).find(function (node) { return name !== '' && (node.id === name || node.getAttribute('name') === name); }) || null;
    };
    function radio(node) { return node.localName === 'input' && node.type.toLowerCase() === 'radio'; }
    function radioValue(node) { return node.hasAttribute('value') ? node.value : 'on'; }
    Object.defineProperty(RadioNodeList.prototype, 'value', {
        get: function () {
            const selected = members(this).find(function (node) { return radio(node) && node.checked; });
            return selected ? radioValue(selected) : '';
        },
        set: function (value) {
            value = String(value);
            const nodes = members(this);
            const selected = nodes.find(function (node) { return radio(node) && radioValue(node) === value; });
            if (selected) { selected.checked = true; }
        }, configurable: true
    });
    Object.defineProperty(HTMLFormElement.prototype, 'elements', {
        get: function () {
            if (!(this instanceof HTMLFormElement)) { throw new TypeError('Invalid form receiver'); }
            if (!collections.has(this)) { collections.set(this, create(this, null)); }
            return collections.get(this);
        }, configurable: true, enumerable: true
    });
    Object.defineProperty(HTMLFormElement.prototype, 'length', {
        get: function () { return this.elements.length; }, configurable: true, enumerable: true
    });
    [HTMLInputElement, HTMLButtonElement, HTMLFieldSetElement, HTMLObjectElement, HTMLOutputElement, HTMLSelectElement, HTMLTextAreaElement].forEach(function (constructor) {
        Object.defineProperty(constructor.prototype, 'form', {
            get: function () {
                const owner = __silksurfControlState(this.nodeId, 0);
                return owner === null ? null : __silksurfWrapNode(owner);
            }, configurable: true, enumerable: true
        });
    });
    Object.defineProperty(HTMLInputElement.prototype, 'checked', {
        get: function () { return __silksurfControlState(this.nodeId, 1); },
        set: function (value) { __silksurfControlState(this.nodeId, 2, !!value); },
        configurable: true, enumerable: true
    });
    globalThis.HTMLCollection = HTMLCollection;
    globalThis.HTMLFormControlsCollection = HTMLFormControlsCollection;
    globalThis.RadioNodeList = RadioNodeList;
})();
