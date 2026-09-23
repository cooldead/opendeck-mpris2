const {test} = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
function inspector(action) {
  const nodes = {}, sent = [];
  const node = id => nodes[id] ||= {value:'', handlers:{}, addEventListener(name, fn) {this.handlers[name]=fn;}, replaceChildren(...children) {this.children=children;}};
  let connection;
  class WebSocket {
    static OPEN=1;
    constructor() {this.readyState=1;connection=this;}
    send(message) {sent.push(JSON.parse(message));}
  }
  const context={window:{},document:{getElementById:node,createElement:()=>({})},WebSocket,setInterval:()=>1,clearInterval:()=>{}};
  vm.runInNewContext(fs.readFileSync('assets/inspector.js','utf8'),context);
  context.window.connectElgatoStreamDeckSocket('1234','ctx','registerPropertyInspector','{}',JSON.stringify({action:`com.cooldead.mpris2.${action}`,payload:{settings:{other:true}}}));
  connection.onopen();
  return {node,sent,connection};
}
test('discovered dropdown changes shared selection; checkbox stays per button',()=>{
  const {node,sent,connection}=inspector('playpause');
  assert.equal(sent.shift().event,'registerPropertyInspector');
  assert.equal(sent.shift().payload.command,'getPlayers');
  connection.onmessage({data:JSON.stringify({event:'sendToPropertyInspector',payload:{selected:'strawberry',players:[{value:'org.mpris.MediaPlayer2.vlc',label:'VLC'}]}})});
  assert.equal(node('player').children.length,3);
  assert.equal(node('player').value,'strawberry');
  assert.equal(node('player').disabled,false);
  node('player').value='org.mpris.MediaPlayer2.vlc'; node('player').handlers.change();
  assert.deepEqual(sent.shift(),{event:'sendToPlugin',action:'com.cooldead.mpris2.playpause',context:'ctx',payload:{command:'selectPlayer',player:'org.mpris.MediaPlayer2.vlc'}});
  node('show-status').checked=true;node('show-status').handlers.change();
  assert.deepEqual(sent.shift(),{event:'setSettings',context:'ctx',payload:{other:true,show_status:true}});
  assert.equal(node('selector').hidden,false); assert.equal(node('status-option').hidden,false);
});
test('artwork exposes selector and transport controls explain shared selection',()=>{
  const art=inspector('artwork');
  assert.equal(art.node('selector').hidden,false);
  assert.equal(art.node('status-option').hidden,true);
  const next=inspector('next');
  assert.equal(next.node('selector').hidden,true);
  assert.equal(next.node('shared-note').hidden,false);
});
test('artwork grid and tile position save per-button settings and restore correctly',()=>{
  const {node,sent,connection}=inspector('artwork');
  sent.length=0;
  assert.equal(node('artwork-layout').hidden,false);
  assert.equal(node('artwork-grid').value,'0');
  node('artwork-grid').value='3';node('artwork-grid').handlers.change();
  assert.equal(node('artwork-position').children.length,9);
  assert.deepEqual(sent.shift().payload,{other:true,artwork_grid:3,artwork_position:0});
  node('artwork-position').value='8';node('artwork-position').handlers.change();
  assert.deepEqual(sent.shift().payload,{other:true,artwork_grid:3,artwork_position:8});
  assert.match(node('tile-map').textContent,/\[9\]/);
  connection.onmessage({data:JSON.stringify({event:'didReceiveSettings',payload:{settings:{artwork_grid:2,artwork_position:3}}})});
  assert.equal(node('artwork-grid').value,'2');assert.equal(node('artwork-position').value,'3');
  assert.equal(node('artwork-position').children.length,4);
  node('artwork-grid').value='1';node('artwork-grid').handlers.change();
  assert.equal(node('tile-settings').hidden,true);
  assert.deepEqual(sent.shift().payload,{artwork_grid:1,artwork_position:0});
});
