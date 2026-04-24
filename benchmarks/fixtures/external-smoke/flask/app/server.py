from flask import Flask, jsonify


def create_app() -> Flask:
    app = Flask(__name__)

    @app.get("/health")
    def health_check():
        return jsonify({"ok": True})

    return app
