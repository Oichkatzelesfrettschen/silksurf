/*
 * react_counter_click -- React 18 useState counter mounted through
 * ReactDOM.createRoot(document.body). bundle_probe drives it with
 * --click inc [--click-repeat n]; every committed re-render writes
 * 'clicks:<count>' into the DOM, so the per-click check asserts
 * document.body.textContent.indexOf('clicks:{n}') >= 0.
 */
var root = ReactDOM.createRoot(document.body);
function Counter() {
  var pair = React.useState(0);
  var count = pair[0], setCount = pair[1];
  return React.createElement('div', null,
    React.createElement('button', {id: 'inc', onClick: function () { setCount(count + 1); }}, 'inc'),
    React.createElement('span', {id: 'out'}, 'clicks:' + count));
}
root.render(React.createElement(Counter));
