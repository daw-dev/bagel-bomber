const field = document.getElementById("field");
const id = Number(field.dataset.id);
let pdr = Number(field.dataset.pdr);
field.removeAttribute("data-pdr");
const dropQueue = [];
const animationTime = 1500;
const styleSheet = document.styleSheets[0];
styleSheet.insertRule(`.field-element { animation-duration: ${animationTime}ms; }`, styleSheet.cssRules.length);
const maxEnvironmentSpawnDelay = 500;

function addElementToField(emoji, isSmall = false) {
    const fieldElement = field.appendChild(document.createElement("div"));
    fieldElement.classList.add("field-element");
    fieldElement.innerHTML = emoji;
    fieldElement.style.setProperty("--field-position", Math.random().toString());
    if (isSmall) {
        fieldElement.classList.add("small");
    }
    setTimeout(() => {
        fieldElement.remove();
    }, animationTime);
    return fieldElement;
}

const dropIntervalId = setInterval(() => {
    if (dropQueue.length > 0) {
        const drop = dropQueue.shift();
        addElementToField(drop.exploded ? "💥" : "🥯").classList.add("drop");
    }
}, 200);


function addEnvironmentEmoji() {
    const emojis = [
        {emoji: "🌲", small: false},
        {emoji: "🌲", small: false},
        {emoji: "🌲", small: false},
        {emoji: "🌳", small: false},
        {emoji: "🌳", small: false},
        {emoji: "🌳", small: false},
        {emoji: "🌴", small: false},
        {emoji: "🌵", small: false},
        {emoji: "🏠", small: false},
        {emoji: "🏡", small: false},
        {emoji: "🏘️", small: false},
        {emoji: "🏫", small: false},
        {emoji: "⛪", small: false},
        {emoji: "🏢", small: false},
        {emoji: "🏦", small: false},
        {emoji: "🌿", small: true},
        {emoji: "🍀", small: true},
        {emoji: "🍁", small: true},
        {emoji: "🍂", small: true},
        {emoji: "🍃", small: true},
        {emoji: "🌺", small: true},
        {emoji: "🌻", small: true},
        {emoji: "🌼", small: true},
        {emoji: "🌷", small: true},
        {emoji: "🌹", small: true},
        {emoji: "🥀", small: true},
        {emoji: "🌸", small: true},
        {emoji: "💐", small: true},
        {emoji: "🌾", small: true},
        {emoji: "🌱", small: true},
        {emoji: "🌰", small: true},
        {emoji: "🍄", small: true},
    ];

    const emoji = emojis[Math.floor(Math.random() * emojis.length)];

    addElementToField(emoji.emoji, emoji.small);
}

function dynamicInterval(callback, intervalGetter) {
    let active = false;

    async function run() {
        active = true;

        while (active) {
            await new Promise(resolve => setTimeout(resolve, intervalGetter()));
            if (active)
                await callback();
        }
    }

    run();

    return () => {
        active = false;
    }
}

dynamicInterval(() => {
    addEnvironmentEmoji();
}, () => Math.random() * maxEnvironmentSpawnDelay * (1 - pdr) + 100);

const hostName = window.location.hostname;
const ws = new WebSocket(`ws://${hostName}:8464`);

ws.onopen = () => {
    console.log("WebSocket connection established.");
    ws.send(id.toString());
};

ws.onmessage = (event) => {
    const data = JSON.parse(event.data);
    pdr = data.pdr;
    const drops = data.drops;
    dropQueue.push(...drops);
};

ws.onerror = () => {
    clearInterval(dropIntervalId);
};

ws.onclose = () => {
    clearInterval(dropIntervalId);
    console.log("WebSocket connection closed.");
};