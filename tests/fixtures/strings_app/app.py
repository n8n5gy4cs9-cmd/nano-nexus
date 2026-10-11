from gettext import gettext as _


def build(window, score):
    window.start = Button(label="Start game")
    window.score.setText("Score: %d points")
    window.hello = Label(text=_("Hello"))
    window.info = Label(text=f"Hi {score}")
    print("starting up")
    log.info("score is %d", score)
    window.tip = Tooltip(title="Press <b>Enter</b> to continue\n", hint='Say "hi"')
    return {"mode": "fast"}
