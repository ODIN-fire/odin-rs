/*
 * Copyright (c) 2023, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The RACE - Runtime for Airspace Concept Evaluation platform is licensed
 * under the Apache License, Version 2.0 (the "License"); you may not use
 * this file except in compliance with the License. You may obtain a copy
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
class LinkedListNode {
    constructor(data, next = null) {
        this.data = data;
        this.next = next;
    }
}

export class LinkedList {
    constructor() {
        this.head = null;
        this.size = 0;
    }

    push(data) {
        this.head = new LinkedListNode(data, this.head);
        this.size++;
    }

    insert(data, cond) {
        this.size++;
        if (this.head) {
            let prevNode = null;
            let node = this.head;
            while (node) {
                if (cond(data, node.data)) {
                    let newNode = new LinkedListNode(data, node);
                    if (prevNode) {
                        prevNode.next = newNode;
                    } else {
                        this.head = newNode;
                    }
                    return;
                }
                prevNode = node;
                node = node.next;
            }
            node.next = new LinkedListNode(data, null);

        } else {
            this.head = new LinkedListNode(data, null);
        }
    }

    //... and more

    *
    [Symbol.iterator]() { // define the default iterator for this class
        let node = this.head; // get first node
        while (node) { // while we have not reached the end of the list
            yield node.data; // ... yield the current node's data
            node = node.next; // and move to the next node
        }
    }

    forEach(f) {
        for (let data of this) f(data);
    }
}

export class TreeNode {
    constructor(data) {
        this.data = data;

        this.parent = undefined;
        this.firstChild = undefined;
        this.nextSibling = undefined;
    }

    level() { // 0-based
        let lvl = 0;
        for (let n = this.parent; n; n = n.parent) lvl++;
        return lvl;
    }

    numberOfChildren() {
        let size = 0;
        this.depthFirstChildren( n=> size++);
        return size;
    }

    //--- set accessors

    parents (){
        let a = [];
        for (let n = this.parent; n; n = n.parent) a.push(n);
        return a.reverse();
    }

    nodeList () {
        let a = [this];
        for (let n = this.parent; n; n = n.parent) a.push(n);
        return a.reverse();
    }

    siblings () {
        let a = [];
        for (let n = this.nextSibling; n; n = n.nextSibling) a.push(n);
        return a;
    }

    children () {
        let a = [];
        for (let n = this.firstChild; n; n = n.nextSibling) a.push(n);
        return a;
    }

    hasChildren() {
        return this.firstChild ? true : false;
    }

    childIndexOf (childNode) {
        let i=0;
        let n = this.firstChild;
        while (!Object.is( n, childNode)) {
            i++;
            n = n.nextSibling;
        }
        return n ? i : -1;
    }

    siblingIndex (){
        let i=0;
        let n = this.parent.firstChild;
        while (n) {
            if (Object.is(n, this)) return i;
            i++;
            n = n.nextSibling;
        }
        throw new Error("corrupted tree");
    }

    //--- modifiers

    addChild (newNode) {
        if (this.firstChild) {
            let n = this.firstChild;
            while (n.nextSibling) n = n.nextSibling;
            n.nextSibling = newNode;
        } else {
            this.firstChild = newNode;
        }
        newNode.parent = this;
        return newNode;
    }

    removeChild (delNode) {
        if (Object.is(this.firstChild,delNode)) {
            this.firstChild = delNode.nextSibling;
        } else {
            let nPrev = this.firstChild;
            for (let n = nPrev.nextSibling; n; n = n.nextSibling) {
                if (Object.is(n, delNode)) {
                    nPrev.nextSibling = delNode.nextSibling;
                    return;
                }
                nPrev = n;
            }
        }
    }

    remove () {
        if (this.parent) {
            this.parent.removeChild(this);
        }
    }

    addSibling (newNode) {
        if (this.nextSibling) {
            let n = this.nextSibling;
            while (n.nextSibling) n = n.nextSibling;
            n.nextSibling = newNode;
        } else {
            this.nextSibling = newNode;
        }
        newNode.parent = this.parent;
        return newNode;
    }

    addToParent (newParentNode) {
        newParentNode.addChild(this);
        return newParentNode;
    }

    // TODO - add insert and remove functions

    //--- traversal functions

    prevSibling() {
        if (parent) {
            let prev = null;
            for (var n = parent.firstChild; n; n = n.nextSibling) {
                if (n == this) return prev;
                prev = n;
            }
        } else {
            return null;
        }
    }

    depthFirst(f) {
        f(this);
        if (this.firstChild) this.firstChild.depthFirst(f);
        if (this.nextSibling) this.nextSibling.depthFirst(f);
    }

    dumpTreeDepthFirst() {
        this.depthFirst( n=>n.show());
    }
    show() {
        console.log(this);
    }

    depthFirstChildren(f) {
        if (this.firstChild) this.firstChild.depthFirst(f);
    }

    depthFirstCond(pred,f) {
        f(this);

        if (pred(this) && this.firstChild) this.firstChild.depthFirstCond(pred,f);
        if (this.nextSibling) this.nextSibling.depthFirstCond(pred,f);
    }

    depthFirstDescendants(pred) {
        let list = [];
        if (pred(this) && this.firstChild) this.firstChild.depthFirstCond( pred, n=> list.push(n));
        return list;
    }

    breadthFirst(f) {
        f(this);
        let level = children();

        while (level.length > 0) {
            let nextLevel = [];
            level.forEach( n=> {
                f(n);
                let ncs = n.children();
                if (ncs.length > 0) nextLevel = nextLevel.concat( ncs);
            });
            level = nextLevel;
        }
    }

    forEach(f) {
        this.depthFirst(f);
    }

    findFirst(pred) {
        if (pred(this)) return this;

        let match = undefined;
        if (this.firstChild) match = this.firstChild.findFirst(pred);
        if (!match && this.nextSibling) match = this.nextSibling.findFirst(pred);

        return match;
    }
}

//--- interactive tree lists

export class ExpandableTreeNode extends TreeNode {

    constructor( name, data, isExpanded, isSticky=false) {
        super(data);
        this.isExpanded = isExpanded;
        this.isSticky = isSticky;
        this.name = name;
    }

    static newRoot() {
        return new ExpandableTreeNode("<root>", null, true);
    }

    static from (items, pathExtractor = (o)=>{return o.pathName;},  branchClassifier = (o)=>{return false;},  stickyClassifier = (o)=>{return false;}, expansionLevel=1) {
        let root = ExpandableTreeNode.newRoot();
        items.forEach( (item) => {
            let pathName = pathExtractor(item);
            if (branchClassifier(item)) { item = null }
            let isSticky = stickyClassifier(item);
            if (pathName) {
                root.sortInPathName(pathName, item)
            }
        });

        root.expandToLevel(expansionLevel);
        return root;
    }

    show() {
        console.log(this.collectNamesUp('/'), this);
    }

    findNode (pathName) {
        let path = pathName.split('/');
        return this.#findNodeRec( path, 0);
    }

    #findNodeRec (path, i) {
        let name = path[i];
        for (let c=this.firstChild; c; c = c.nextSibling) {
            if (c.name == name) {
                i++;
                if (i >= path.length) {
                    return c;
                } else {
                    return c.#findNodeRec( path, i)
                }
            }
        }
        return null; // not found
    }

    sortInPathName(pathName, newData, expand=false, isSticky=false) {
        let path = pathName.split('/');
        let newNode = new ExpandableTreeNode( path[path.length-1], newData, expand, isSticky);

        return  this.#insert(0,path,newNode); // this returns all nodes that were added
    }

    removePathName(pathName) {
        let removedNodes = [];
        let node = this.findNode(pathName);
        if (node) {
            node.remove();
            removedNodes.push(node);
            
            node = node.parent;
            while (node && !node.hasChildren() && !node.data && !node.isSticky){ 
                node.remove();
                removedNodes.push(node);
                node = node.parent;
            }
        }
        return removedNodes;
    }

    setPathNameSticky (pathName, isSticky=true) {
        let node = this.findNode(pathName);
        if (node) {
            node.isSticky = isSticky;
        }
    }

    setSticky (cond) {
        this.isSticky = cond;
    }

    isVisible () {
        let p = this.parent;
        while (p) {
            if (!p.isExpanded) return false;
            p = p.parent;
        }
        return true;
    }

    // the closest node with an expanded parent
    nearestVisible () { 
        let n = this;
        let p = this.parent;
        while (p) {
            if (p.isExpanded) return n;
            n = p;
            p = p.parent;
        }
        return null;
    }

    visibleIndex () {
        let n = this;
        while (n.parent) {
            n = n.parent;
            if (!n.isExpanded) return -1; // collapsed parent means not visible
        }

        let res = {found:false, i: -1};
        n.#countExpandedTo(this, res);

        return res.found ? res.i : -1;
    }

    #countExpandedTo(endNode, res) {
        if (Object.is(this, endNode)) { res.found = true; return; }
        res.i++;

        if (this.firstChild && this.isExpanded) {
            for (let n = this.firstChild; n; n = n.nextSibling) {
                n.#countExpandedTo(endNode, res);
                if (res.found) return;
            }
        }
    }

    static fromPreOrdered (items, pathExtractor = o=>o.pathName, expansionLevel=1) {
        let root = ExpandableTreeNode.newRoot();

        items.forEach( data=> {
            let pathName = pathExtractor(data);
            let path = pathName.split("/").reverse();
            root.addPath(path, data);
        });

        root.expandToLevel(expansionLevel);
        return root;
    }

    addPath(path, newData) {
        let p = path[path.length-1]; // path is in reverse order, from leaf to root

        for (let c=this.firstChild; c; c = c.nextSibling) {
            if (c.name == p) {
                path.pop();
                c.addPath(path, newData);
                return;
            }
        }

        let node = new ExpandableTreeNode(path[0], newData, false);
        path.splice(0,1);
        node = path.reduce( (prev,n) => prev.addToParent( new ExpandableTreeNode(n,null,false)), node);
        this.addChild(node);
    }

    expandToLevel (l) {
        let lmax = this.level() + l;

        this.depthFirstChildren( n=> {
            n.isExpanded = n.level() <= lmax;
        })
    }

    nodePrefix() {
        let lvl = this.level() -1;
        //let endChar = this.firstChild ? (this.isExpaned ? '▽ '  : '▷ ') : '· ';
        let endChar = this.firstChild ? (this.isExpanded ? '▼ '  : '▶︎ ') : '· ';

        if (lvl < 1) return endChar;
        else if (lvl == 1) return '· ' + endChar;
        else return '· '.repeat(lvl) + endChar;
    }

    #insert(lvl, path, newNode, addedNodes=[]) {
        if (lvl == path.length-1) {
            this.sortInChild(newNode);
            addedNodes.push(newNode);
            return addedNodes;

        } else {
            let head = path[lvl];
            for (var c = this.firstChild; c; c = c.nextSibling) {
                if (c.name == head) {
                    return c.#insert(lvl+1, path, newNode, addedNodes);
                }
            }
            // head does not match any of our children, sort in remaining chain
            let pn = new ExpandableTreeNode(head, null, this.isExpaned);
            this.sortInChild(pn);
            addedNodes.push(pn);

            for (let i = lvl+1; i < path.length-1; i++) {
                let cn = new ExpandableTreeNode(path[i], null, newNode.isExpaned);
                addedNodes.push( cn);
                pn.firstChild = cn;
                cn.parent = pn;
                pn = cn;
            }
            pn.firstChild = newNode;
            newNode.parent = pn;
            addedNodes.push(newNode);

            return addedNodes;
        }
    }

    sortInChild (newNode) {
        newNode.parent = this;
    
        var prev = undefined;
        for (var n = this.firstChild; n && n.name < newNode.name; n = n.nextSibling) prev = n;
        if (prev) {
            newNode.nextSibling = prev.nextSibling;
            prev.nextSibling = newNode;
        } else {
            newNode.nextSibling = this.firstChild;
            this.firstChild = newNode;
        }
    } 

    setExpanded (cond) {
        this.isExpanded = cond;
    }

    expand() {
        this.isExpanded = true;
    }

    collapse() {
        this.isExpanded = false;
    }

    expandAll() {
        this.depthFirstChildren( n=> n.isExpanded = true);
    }

    expandChildren() {
        for (let n=this.firstChild; n; n = n.nextSibling) n.isExpanded = true;
    }

    expandParents() {
        for (let n=this.parent; n; n = n.parent) n.isExpanded = true;
    }

    collapseAll() {
        this.depthFirstChildren( n=> n.isExpanded = false);
    }

    collapseChildren() {
        for (let n=this.firstChild; n; n = n.nextSibling) n.isExpanded = false;
    }

    expandedDescendants() {
        return this.depthFirstDescendants( n=> n.isExpanded);
    }

    collectNamesUp (sep) {
        let path = this.name;
        let n = this.parent;
        while (n) {
            if (n.parent) { // skip artificial <ROOT> node
                path = n.name + sep + path;
            }
            n = n.parent;
        }
        return path;
    }
}


class SkipListNode {
    constructor(data, next) {
        this.data = data;
        this.next = next;

        this.width = new Array(next.length);
        this.width.fill(0);
    }
}

//const MaxSkipListDepth = 16;
const MaxSkipListDepth = 5;

export class SkipList {
    constructor(depth, isBefore, isSame) {
        this.depth = depth;
        this.isBefore = isBefore;
        this.isSame = isSame;

        this.head = new SkipListNode(null, new Array(depth));
        this.size = 0;
        this.maxLevel = 0; // index - there always is at least one level
    }

    get length() {
        return this.size;
    }

    indexOf(data) {
        let idx = 0;
        let n = this.head;
        for (let lvl = this.maxLevel; lvl >= 0; lvl--) {
            while (n.next[lvl] && this.isBefore(n.next[lvl].data, data)) {
                idx += n.width[lvl];
                n = n.next[lvl];
            }
        }
        n = n.next[0];
        return (n && this.isSame(data, n.data)) ? idx : -1;
    }

    includes(data) {
        return this.indexOf(data) != -1;
    }

    randomLevel() {
        var lvl = 0;
        while (lvl < this.depth && Math.random() < 0.5) lvl++;
        return lvl;
    }

    insert(data, insertFunc, updateFunc) {
        let idx = 0; // eventually holds level 0 index of new element
        let nodeLevel = 0;
        let update = new Array(this.maxLevel + 1);
        let updateIdx = new Array(update.length); // index of update node
        let n = this.head;

        for (let lvl = this.maxLevel; lvl >= 0; lvl--) {
            while (n.next[lvl] && this.isBefore(n.next[lvl].data, data)) {
                idx += n.width[lvl];
                n = n.next[lvl];
            }
            update[lvl] = n;
            updateIdx[lvl] = idx - 1;
        }

        n = n.next[0];
        if (n && this.isSame(n.data, data)) { // update (keep it similar to Map)
            let oldData = n.data;
            n.data = data;
            if (updateFunc) updateFunc(idx, oldData);

        } else { // insert or append
            nodeLevel = this.randomLevel();
            n = new SkipListNode(data, new Array(nodeLevel + 1));
            let oldMaxLevel = this.maxLevel;

            if (nodeLevel > oldMaxLevel) { // first node of this level, link new skip lanes from head
                for (let lvl = oldMaxLevel + 1; lvl <= nodeLevel; lvl++) {
                    this.head.next[lvl] = n;
                    this.head.width[lvl] = idx + 1;
                }
                this.maxLevel = nodeLevel;
            }

            //--- base lane update
            n.next[0] = update[0].next[0];
            update[0].next[0] = n;
            update[0].width[0] = 1;
            if (n.next[0]) n.width[0] = 1;

            //--- pre-existing skip lane updates
            for (let lvl = 1; lvl <= oldMaxLevel; lvl++) {
                let u = update[lvl];
                let uw = u.width[lvl];

                if (lvl > nodeLevel) { // u is above our level chain
                    if (uw > 0) u.width[lvl]++;

                } else { // insert into level chain
                    n.next[lvl] = u.next[lvl];
                    u.next[lvl] = n;

                    let uIdx = updateIdx[lvl];
                    u.width[lvl] = (idx - uIdx);
                    if (uw > 0) n.width[lvl] = uw - (idx - uIdx) + 1;
                }
            }

            this.size++;
            if (insertFunc) insertFunc(idx);
        }

        return idx;
    }

    remove(data) {
        let update = [];
        let n = this.head;

        for (let lvl = this.maxLevel; lvl >= 0; lvl--) {
            while (n.next[lvl] && this.isBefore(n.next[lvl].data, data)) n = n.next[lvl];
            update[lvl] = n;
        }

        n = n.next[0];
        if (n && this.isSame(n.data, data)) {
            //--- update skip lane widths
            for (let lvl = 1; lvl <= this.maxLevel; lvl++) {
                let u = update[lvl];
                let uw = u.width[lvl];
                let nodeLevel = n.next.length - 1;

                if (lvl > nodeLevel) { // above our level chain
                    if (uw > 0) u.width[lvl]--;
                } else {
                    if (n.next[lvl]) u.width[lvl] += n.width[lvl] - 1;
                    else u.width[lvl] = 0;
                }
            }
            if (n.next[0] == null) update[0].width[0] = 0;

            for (let lvl = 0; lvl <= this.maxLevel && update[lvl].next[lvl] === n; lvl++) {
                update[lvl].next[lvl] = n.next[lvl];
            }
            for (let lvl = this.maxLevel; lvl >= 0 && this.head.next[lvl] == null; lvl--) this.maxLevel--;

            this.size--;
            return true;

        } else { // nothing to remove
            return false;
        }
    }

    at(idx) {
        let n = this.head;
        idx++;
        for (let lvl = this.maxLevel; lvl >= 0; lvl--) {
            while (n.width[lvl] && idx >= n.width[lvl]) {
                idx -= n.width[lvl];
                if (idx == 0) {
                    let nn = n.next[lvl];
                    return nn ? nn.data : null;
                }
                n = n.next[lvl];
            }
        }
        return null;
    }

    *
    [Symbol.iterator]() {
        if (this.size > 0) {
            let n = this.head.next[0];
            while (n) {
                yield n.data;
                n = n.next[0];
            }
        }
    }

    forEach(f) {
        for (let data of this) f(data);
    }

    clear() {
        for (let lvl = this.maxLevel; lvl >= 0; lvl--) this.head.next[lvl] = null;
        this.size = 0;
        this.maxLevel = 0;
    }

    //--- debug funcs

    toString() {
        let s = "[";
        let i = 0;
        for (let data of this) {
            if (i > 0) s += ',';
            s += JSON.stringify(data);
            i++;
        }
        s += ']';
        return s;
    }

    checkOrder(labelFunc) {
        let prev = null;
        let n = this.head.next[0];
        for (let i = 0; i < this.size; i++) {
            if (prev) {
                if (!this.isBefore(prev.data, n.data)) {
                    console.log("!!! wrong order at index: " + i);
                    if (labelFunc) this.dumpOrder(labelFunc, i + 1);

                    return false;
                }
            }
            prev = n;
            n = n.next[0];
        }
        return true;
    }

    dumpOrder(labelFunc, i = this.size) {
        let n = this.head.next[0];
        for (let j = 0; j < i; j++) {
            console.log("  " + j + ": " + labelFunc(n.data));
            n = n.next[0];
        }
    }

    dump(colWidth = 3, labelFunc = undefined) {
        function toString(x) {
            if (x != null) {
                let s = labelFunc ? labelFunc(x) : x.toString();
                if (s.length < colWidth) s = " ".repeat(colWidth - s.length) + s;
                return s;
            } else return "  -";
        }

        function nid(n) {
            if (n) return "(" + toString(n.data) + ")";
            else return "     ";
        }

        function printNode(prefix, n) {
            let line = prefix + ": (" + toString(n.data) + ") : ";
            for (let i = 0; i < n.next.length; i++) line += " ".repeat(colWidth) + nid(n.next[i]) + "+" + (n.width[i]);
            console.log(line);
        }

        console.log("-------------------------------------------------------------------- size " + this.size);
        printNode(" hd", this.head);
        let n = this.head.next[0];
        for (let i = 0; i < this.size; i++) {
            printNode(toString(i), n);
            n = n.next[0];
        }
    }
}

export class CircularBuffer {
    constructor(maxSize) {
        this.maxSize = maxSize;
        this.size = 0;
        this.i0 = -1;
        this.i1 = -1;
        this.buffer = [];
    }

    push(v) {
        this.i1++;
        this.i1 %= this.maxSize;
        this.buffer[this.i1] = v;

        if (this.size < this.maxSize) {
            if (this.size == 0) this.i0 = 0;
            this.size++;
        } else {
            this.i0++;
            this.i0 %= this.maxSize;
        }
    }

    dropLast(n) {
        if (n > 0) {
            if (n >= this.size) this.clear();
            else {
                this.size -= n;
                this.i1 -= n;
                if (this.i1 < 0) this.i1 += this.maxSize;
            }
        }
    }

    dropFirst(n) {
        if (n > 0) {
            if (n >= this.size) this.clear();
            else {
                this.size -= n;
                this.i0 += n;
                this.i0 %= this.maxSize;
            }
        }
    }

    at(i) {
        if (i >= 0 && i < this.size) {
            let idx = (this.i0 + i) % this.maxSize;
            return this.buffer[idx];
        } else return undefined;
    }


    reverseAt(i) {
        if (i >= 0 && i < this.size) {
            let idx = (this.i1 - i);
            if (idx < 0) idx = this.maxSize + idx;
            return this.buffer[idx];
        } else return undefined;
    }

    *
    [Symbol.iterator]() {
        if (this.size > 0) {
            let i = this.i0;
            while (true) {
                yield this.buffer[i];
                if (i == this.i1) break;
                i++;
                i %= this.maxSize;
            }
        }
    }

    forEach(f) {
        for (let v of this) f(v);
    }

    first() {
        if (this.size > 0) return this.buffer[this.i0];
        else return undefined;
    }

    last() {
        if (this.size > 0) return this.buffer[this.i1];
        else return undefined;
    }

    clear() {
        this.size = 0;
        this.i0 = -1;
        this.i1 = -1;
        this.buffer = [];
    }

    toString() {
        let s = "[";
        let i = 0;
        for (let data of this) {
            if (i > 0) s += ',';
            s += JSON.stringify(data);
            i++;
        }
        s += ']';
        return s;
    }
}

//--- simple seeded PRNG to support reproducible values for testing (lifted from http://pracrand.sourceforge.net/)

function _sfc32(a, b, c, d) {
    return function() {
        a >>>= 0;
        b >>>= 0;
        c >>>= 0;
        d >>>= 0;
        let t = (a + b) | 0;
        a = b ^ b >>> 9;
        b = c + (c << 3) | 0;
        c = (c << 21 | c >>> 11);
        d = d + 1 | 0;
        t = t + d | 0;
        c = c + t | 0;
        return (t >>> 0) / 4294967296;
    }
}

var _rand = _sfc32(0x9E3779B9, 0x243F6A88, 0xB7E15162, (1337 ^ 0xDEADBEEF));
for (let i = 0; i < 15; i++) _rand(); // warm up shuffle

// this is a drop-in replacement for math.random() which is non-seedable
export function pseudoRandom() {
    return _rand();
}
