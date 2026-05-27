function getGreetingParts(hour) {
    if (hour < 5) return ["Good night", "🌙"];
    if (hour < 12) return ["Good morning", "☀️"];
    if (hour < 17) return ["Good afternoon", "🌤️"];
    if (hour < 21) return ["Good evening", "🌆"];
    return ["Good night", "🌙"];
}

export function updateHeroGreeting() {
    const greetingEl = document.getElementById("heroGreeting");
    if (!greetingEl) return;

    const [greeting, emoji] = getGreetingParts(new Date().getHours());
    greetingEl.textContent = `${greeting} ${emoji}`;
}

export function updateHeroSubtitle() {
    const subEl = document.getElementById("heroSub");
    if (!subEl) return;

    const now = new Date();
    const date = now.toLocaleDateString(undefined, {
        weekday: "long",
        year: "numeric",
        month: "long",
        day: "numeric",
    });
    const time = now.toLocaleTimeString(undefined, {
        hour: "numeric",
        minute: "2-digit",
        second: "2-digit",
        hour12: true,
    });

    subEl.textContent = `${date} · ${time}`;
}
