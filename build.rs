fn main() {
    // Icône du fichier .exe (Explorateur, barre des tâches, Alt+Tab).
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");
    embed_resource::compile("assets/app.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();

    // Embarque le runtime Windows App SDK : les DLL sont copiées à côté de l'exécutable et un
    // manifeste « autonome » est lié. L'appli n'exige alors plus d'installer le runtime.
    // Au premier build, les paquets NuGet (≈ 70 Mo) sont téléchargés puis mis en cache.
    windows_reactor_setup::as_self_contained();
}
