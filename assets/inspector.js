'use strict';
let socket, context, actionUUID, settings = {}, timer, optionsKey = '';
const element = id => document.getElementById(id);
const input = element('player');
const status = element('status');
const showStatus = element('show-status');
const gridInput = element('artwork-grid');
const positionInput = element('artwork-position');
function send(payload) {
  if (socket?.readyState === WebSocket.OPEN)
    socket.send(JSON.stringify({event:'sendToPlugin', action:actionUUID, context, payload}));
}
function displayLayout() {
  const configured = Number(settings.artwork_grid ?? 0);
  const grid = [1, 2, 3].includes(configured) ? configured : 0;
  const tileGrid = grid || 1;
  const position = Math.max(0, Math.min(Number(settings.artwork_position) || 0, Math.max(0, grid * grid - 1)));
  gridInput.value = String(grid);
  element('artwork-auto-note').hidden = grid !== 0;
  element('tile-settings').hidden = grid === 0 || grid === 1;
  positionInput.replaceChildren(...Array.from({length:Math.max(1, grid * grid)}, (_, index) => {
    const option = document.createElement('option');
    option.value = String(index);
    option.textContent = `Tile ${index + 1} — row ${Math.floor(index / tileGrid) + 1}, column ${index % tileGrid + 1}`;
    return option;
  }));
  positionInput.value = String(position);
  element('tile-map').textContent = Array.from({length:grid}, (_, row) =>
    Array.from({length:grid}, (_, col) => {
      const index = row * grid + col;
      return index === position ? `[${index + 1}]` : ` ${index + 1} `;
    }).join(' ')).join('\n');
}
function saveSettings() {
  if (socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({event:'setSettings', context, payload:settings}));
}
function displaySettings(value) {
  settings = value || {};
  showStatus.checked = settings.show_status === true;
  displayLayout();
}
function displayPlayers(payload) {
  if (typeof payload.selected !== 'string') {
    status.textContent = 'Loading saved player selection…';
    return;
  }
  const entries = [{value:'auto',label:'Automatic (prefer playing)'}, ...(payload.players || [])];
  if (!entries.some(item => item.value === payload.selected)) {
    const matched = entries.find(item => item.value === `org.mpris.MediaPlayer2.${payload.selected}`);
    entries.push({value:payload.selected, label: matched ? matched.label : `${payload.selected} (not running)`});
  }
  const nextKey = JSON.stringify(entries);
  if (nextKey !== optionsKey) {
    optionsKey = nextKey;
    input.replaceChildren(...entries.map(item => {
    const option = document.createElement('option');
    option.value = item.value;
    // Bus suffix distinguishes multiple instances of the same application.
    option.textContent = item.value.startsWith('org.mpris.MediaPlayer2.')
      ? `${item.label} — ${item.value.slice('org.mpris.MediaPlayer2.'.length)}` : item.label;
    return option;
    }));
  }
  input.value = payload.selected;
  input.disabled = false;
  status.textContent = payload.error || 'Selection applies to every MPRIS2 Media control.';
}
window.connectElgatoStreamDeckSocket = function(port, uuid, registerEvent, info, actionInfo) {
  context = uuid;
  const action = JSON.parse(actionInfo);
  actionUUID = action.action;
  const selector = actionUUID.endsWith('.artwork') || actionUUID.endsWith('.playpause');
  element('selector').hidden = !selector;
  element('shared-note').hidden = selector;
  element('status-option').hidden = !actionUUID.endsWith('.playpause');
  element('artwork-note').hidden = !actionUUID.endsWith('.artwork');
  element('artwork-layout').hidden = !actionUUID.endsWith('.artwork');
  displaySettings(action.payload?.settings);
  socket = new WebSocket(`ws://127.0.0.1:${port}`);
  socket.onopen = () => {
    socket.send(JSON.stringify({event:registerEvent, uuid}));
    send({command:'getPlayers'});
    timer = setInterval(() => send({command:'getPlayers'}), 3000);
  };
  socket.onmessage = ({data}) => {
    const message = JSON.parse(data);
    if (message.event === 'didReceiveSettings') displaySettings(message.payload.settings);
    if (message.event === 'sendToPropertyInspector') displayPlayers(message.payload);
  };
  socket.onclose = () => { clearInterval(timer); input.disabled = true; status.textContent = 'Disconnected from OpenDeck.'; };
};
input.addEventListener('change', () => {
  input.disabled = true;
  status.textContent = 'Saving player selection…';
  send({command:'selectPlayer', player:input.value});
});
element('refresh').addEventListener('click', () => send({command:'getPlayers'}));
showStatus.addEventListener('change', () => {
  if (socket?.readyState !== WebSocket.OPEN) return;
  settings = {...settings, show_status:showStatus.checked};
  socket.send(JSON.stringify({event:'setSettings', context, payload:settings}));
});

gridInput.addEventListener('change', () => {
  settings = {...settings, artwork_grid:Number(gridInput.value), artwork_position:0};
  displayLayout();
  saveSettings();
});
positionInput.addEventListener('change', () => {
  settings = {...settings, artwork_position:Number(positionInput.value)};
  displayLayout();
  saveSettings();
});
