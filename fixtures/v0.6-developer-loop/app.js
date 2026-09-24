// Intentional deterministic defect for the V0.6 physical developer-loop test.
// The acceptance task should patch this to `true` through latch_files.
const animationFixed = false;

const state = document.querySelector('#animation-state');
if (animationFixed) {
  state.dataset.animationState = 'fixed';
  state.textContent = 'Landing animation: fixed';
}

const button = document.querySelector('#test-button');
const result = document.querySelector('#result');
button.addEventListener('click', async () => {
  const response = await fetch('/api/ping', { method: 'POST' });
  const payload = await response.json();
  result.textContent = payload.status;
  result.dataset.requestId = payload.request_id;
  console.log('latch-fixture-clicked', payload.request_id);
});
