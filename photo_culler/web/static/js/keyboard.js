/* Keyboard navigation shortcuts for rapid culling inspector */
document.addEventListener('keydown', function(e) {
  if (['input', 'textarea'].includes(document.activeElement.tagName.toLowerCase())) {
    return;
  }

  const photoId = document.body.dataset.currentPhotoId;
  if (!photoId) return;

  let decision = null;
  switch(e.key) {
    case '1': decision = 'best'; break;
    case '2': decision = 'keep'; break;
    case '3': decision = 'alternate'; break;
    case '4': decision = 'review'; break;
    case 'x': case 'X': decision = 'reject'; break;
    case 'r': case 'R': decision = 'recover'; break;
  }

  if (decision) {
    const btn = document.querySelector(`[data-decision="${decision}"]`);
    if (btn) btn.click();
  }
});
