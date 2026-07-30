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

  // Handle Left and Right Arrow navigation
  if (e.key === 'ArrowLeft') {
    const headerBar = document.querySelector('.header-bar');
    if (headerBar) {
      const prevId = headerBar.dataset.prevPhotoId;
      if (prevId) {
        const group = headerBar.dataset.groupId;
        window.location.href = `/photos/${prevId}${group ? `?group=${encodeURIComponent(group)}` : ''}`;
      }
    }
  } else if (e.key === 'ArrowRight') {
    const headerBar = document.querySelector('.header-bar');
    if (headerBar) {
      const nextId = headerBar.dataset.nextPhotoId;
      if (nextId) {
        const group = headerBar.dataset.groupId;
        window.location.href = `/photos/${nextId}${group ? `?group=${encodeURIComponent(group)}` : ''}`;
      }
    }
  }
});
