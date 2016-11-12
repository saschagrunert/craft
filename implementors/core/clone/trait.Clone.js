(function() {var implementors = {};
implementors["craft"] = ["impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/dependency/struct.Dependency.html' title='craft::dependency::Dependency'>Dependency</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/dependency/struct.DependencyInner.html' title='craft::dependency::DependencyInner'>DependencyInner</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/dependency/enum.Platform.html' title='craft::dependency::Platform'>Platform</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/dependency/enum.Kind.html' title='craft::dependency::Kind'>Kind</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/manifest/struct.Manifest.html' title='craft::manifest::Manifest'>Manifest</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/manifest/struct.VirtualManifest.html' title='craft::manifest::VirtualManifest'>VirtualManifest</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/manifest/struct.ManifestMetadata.html' title='craft::manifest::ManifestMetadata'>ManifestMetadata</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/manifest/enum.LibKind.html' title='craft::manifest::LibKind'>LibKind</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/manifest/enum.TargetKind.html' title='craft::manifest::TargetKind'>TargetKind</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/manifest/struct.Profile.html' title='craft::manifest::Profile'>Profile</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/manifest/struct.Profiles.html' title='craft::manifest::Profiles'>Profiles</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/manifest/struct.Target.html' title='craft::manifest::Target'>Target</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/ops/enum.CompileMode.html' title='craft::ops::CompileMode'>CompileMode</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/ops/enum.MessageFormat.html' title='craft::ops::MessageFormat'>MessageFormat</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/ops/enum.VersionControl.html' title='craft::ops::VersionControl'>VersionControl</a>","impl&lt;'a&gt; <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/ops/struct.Unit.html' title='craft::ops::Unit'>Unit</a>&lt;'a&gt;","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/ops/struct.BuildOutput.html' title='craft::ops::BuildOutput'>BuildOutput</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/ops/enum.Kind.html' title='craft::ops::Kind'>Kind</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/ops/struct.BuildConfig.html' title='craft::ops::BuildConfig'>BuildConfig</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/ops/struct.TargetConfig.html' title='craft::ops::TargetConfig'>TargetConfig</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/package/struct.Package.html' title='craft::package::Package'>Package</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/package_id/struct.PackageId.html' title='craft::package_id::PackageId'>PackageId</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/package_id/enum.PackageIdError.html' title='craft::package_id::PackageIdError'>PackageIdError</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/package_id/struct.Metadata.html' title='craft::package_id::Metadata'>Metadata</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/package_id_spec/struct.PackageIdSpec.html' title='craft::package_id_spec::PackageIdSpec'>PackageIdSpec</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/resolver/struct.EncodablePackageId.html' title='craft::resolver::EncodablePackageId'>EncodablePackageId</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/resolver/struct.Resolve.html' title='craft::resolver::Resolve'>Resolve</a>","impl&lt;'a&gt; <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/resolver/enum.Method.html' title='craft::resolver::Method'>Method</a>&lt;'a&gt;","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/shell/enum.Verbosity.html' title='craft::shell::Verbosity'>Verbosity</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/shell/enum.ColorConfig.html' title='craft::shell::ColorConfig'>ColorConfig</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/shell/struct.ShellConfig.html' title='craft::shell::ShellConfig'>ShellConfig</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/source/enum.GitReference.html' title='craft::source::GitReference'>GitReference</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/source/struct.SourceId.html' title='craft::source::SourceId'>SourceId</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/sources/git/struct.GitRevision.html' title='craft::sources::git::GitRevision'>GitRevision</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/sources/git/struct.GitRemote.html' title='craft::sources::git::GitRemote'>GitRemote</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/summary/struct.Summary.html' title='craft::summary::Summary'>Summary</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/util/config/enum.Location.html' title='craft::util::config::Location'>Location</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/util/config/enum.ConfigValue.html' title='craft::util::config::ConfigValue'>ConfigValue</a>","impl&lt;N:&nbsp;<a class='trait' href='https://doc.rust-lang.org/nightly/core/cmp/trait.Eq.html' title='core::cmp::Eq'>Eq</a> + <a class='trait' href='https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html' title='core::hash::Hash'>Hash</a> + <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a>&gt; <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/graph/struct.Graph.html' title='craft::util::graph::Graph'>Graph</a>&lt;N&gt;","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/process_builder/struct.ProcessBuilder.html' title='craft::util::process_builder::ProcessBuilder'>ProcessBuilder</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/toml/struct.Layout.html' title='craft::util::toml::Layout'>Layout</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/toml/struct.DetailedTomlDependency.html' title='craft::util::toml::DetailedTomlDependency'>DetailedTomlDependency</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/toml/struct.TomlProfiles.html' title='craft::util::toml::TomlProfiles'>TomlProfiles</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/toml/struct.TomlOptLevel.html' title='craft::util::toml::TomlOptLevel'>TomlOptLevel</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/toml/struct.TomlProfile.html' title='craft::util::toml::TomlProfile'>TomlProfile</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/util/enum.Cfg.html' title='craft::util::Cfg'>Cfg</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/util/enum.CfgExpr.html' title='craft::util::CfgExpr'>CfgExpr</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/util/enum.Freshness.html' title='craft::util::Freshness'>Freshness</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='struct' href='craft/util/struct.Filesystem.html' title='craft::util::Filesystem'>Filesystem</a>","impl <a class='trait' href='https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html' title='core::clone::Clone'>Clone</a> for <a class='enum' href='craft/workspace/enum.WorkspaceConfig.html' title='craft::workspace::WorkspaceConfig'>WorkspaceConfig</a>",];

            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        
})()
